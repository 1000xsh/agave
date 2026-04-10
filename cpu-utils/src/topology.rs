//! CPU topology detection and physical core management.

#[cfg(target_os = "linux")]
use {
    crate::affinity::{CpuId, cpu_count, online_cpus, set_cpu_affinity},
    std::{fs, io},
};
use {crate::error::CpuAffinityError, std::collections::BTreeMap};

/// Identifies a physical CPU core by its package (socket) and core ID.
///
/// `core_id` from sysfs is only unique within a socket, so the composite
/// `(package_id, core_id)` is needed to uniquely identify a physical core
/// across multi-socket systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PhysicalCpuId {
    pub package_id: usize,
    pub core_id: usize,
}

/// Get the number of physical CPU cores (excluding hyperthreads).
///
/// Reads CPU topology from sysfs to count unique physical cores. Falls back to
/// the logical CPU count when topology files are absent (e.g., containers or
/// VMs that do not expose sysfs topology).
///
/// # Examples
///
/// ```no_run
/// # use agave_cpu_utils::*;
/// # fn main() -> Result<(), CpuAffinityError> {
/// let physical = physical_core_count()?;
/// let logical = cpu_count()?;
/// println!("Physical cores: {}, Logical CPUs: {}", physical, logical);
/// if logical > physical {
///     println!("Hyperthreading enabled ({}x)", logical / physical);
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns [`CpuAffinityError::Io`] if unable to read system information.
/// Returns [`CpuAffinityError::ParseError`] if sysfs topology data is malformed.
/// Returns [`CpuAffinityError::NotSupported`] on non-Linux platforms.
#[cfg(target_os = "linux")]
pub fn physical_core_count() -> Result<usize, CpuAffinityError> {
    match core_to_cpus_mapping() {
        Ok(mapping) => Ok(mapping.len()),
        Err(CpuAffinityError::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => cpu_count(),
        Err(e) => Err(e),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn physical_core_count() -> Result<usize, CpuAffinityError> {
    Err(CpuAffinityError::NotSupported)
}

/// Get a mapping of physical core IDs to logical CPU IDs.
///
/// Uses `(package_id, core_id)` as the composite key to uniquely identify
/// physical cores across multi-socket systems. Iterates only online CPUs.
///
/// Returns an error if any online CPU is missing topology files in sysfs.
/// Callers that want to handle partial topology (e.g., containers) should
/// check for [`CpuAffinityError::Io`] with [`std::io::ErrorKind::NotFound`]
/// and fall back as appropriate.
///
/// # Returns
/// A `BTreeMap` where keys are [`PhysicalCpuId`] and values are sorted vectors
/// of [`CpuId`] belonging to each physical core.
///
/// # Examples
///
/// ```no_run
/// # use agave_cpu_utils::*;
/// # fn main() -> Result<(), CpuAffinityError> {
/// let mapping = core_to_cpus_mapping()?;
/// for (phys, cpus) in &mapping {
///     println!("Package {} Core {} -> CPUs {:?}", phys.package_id, phys.core_id, cpus);
/// }
/// # Ok(())
/// # }
/// ```
///
/// On a system with hyperthreading, a physical core might map to `[CpuId(0), CpuId(4)]`.
///
/// # Errors
///
/// Returns [`CpuAffinityError::Io`] if unable to read topology information, including
/// if topology files are absent for any online CPU.
/// Returns [`CpuAffinityError::ParseError`] if sysfs topology data is malformed.
/// Returns [`CpuAffinityError::NotSupported`] on non-Linux platforms.
#[cfg(target_os = "linux")]
pub fn core_to_cpus_mapping() -> Result<BTreeMap<PhysicalCpuId, Vec<CpuId>>, CpuAffinityError> {
    let mut mapping: BTreeMap<PhysicalCpuId, Vec<CpuId>> = BTreeMap::new();

    for cpu in online_cpus()? {
        let core_id_path = format!("/sys/devices/system/cpu/cpu{cpu}/topology/core_id");
        let package_id_path =
            format!("/sys/devices/system/cpu/cpu{cpu}/topology/physical_package_id");

        let core_id = fs::read_to_string(&core_id_path)
            .map_err(CpuAffinityError::Io)?
            .trim()
            .parse::<usize>()
            .map_err(|_| CpuAffinityError::ParseError(format!("invalid core_id for cpu{cpu}")))?;

        let package_id = fs::read_to_string(&package_id_path)
            .map_err(CpuAffinityError::Io)?
            .trim()
            .parse::<usize>()
            .map_err(|_| {
                CpuAffinityError::ParseError(format!("invalid physical_package_id for cpu{cpu}"))
            })?;

        mapping
            .entry(PhysicalCpuId {
                package_id,
                core_id,
            })
            .or_default()
            .push(cpu);
    }

    // Sort CPU lists for consistency
    for cpus in mapping.values_mut() {
        cpus.sort_unstable();
    }

    Ok(mapping)
}

#[cfg(not(target_os = "linux"))]
pub fn core_to_cpus_mapping() -> Result<BTreeMap<PhysicalCpuId, Vec<CpuId>>, CpuAffinityError> {
    Err(CpuAffinityError::NotSupported)
}

/// Set CPU affinity using only physical cores (avoiding hyperthreads).
///
/// Pins the thread to the first (lowest-numbered) logical CPU of each specified
/// physical core, avoiding hyperthreading siblings. Useful for reducing performance
/// variance and cache contention in performance-critical threads.
///
/// In cpuset-restricted environments (e.g., containers), the first logical CPU of
/// a physical core may not be in the allowed set even if another sibling is. In that
/// case the function returns [`CpuAffinityError::Io`] with `EINVAL`.
///
/// # Arguments
/// * `cores` - Physical core IDs to bind to
///
/// # Examples
///
/// ```no_run
/// # use agave_cpu_utils::*;
/// # fn main() -> Result<(), CpuAffinityError> {
/// let mapping = core_to_cpus_mapping()?;
/// let first_core = mapping.keys().next().copied().unwrap();
/// set_cpu_affinity_physical(None, [first_core])?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns [`CpuAffinityError::EmptyCpuList`] if the core list is empty.
/// Returns [`CpuAffinityError::InvalidPhysicalCore`] if any core ID is not found in topology.
/// Returns [`CpuAffinityError::Io`] if the system call fails, including `EINVAL` in
/// cpuset-restricted environments where the first sibling is outside the allowed set.
/// Returns [`CpuAffinityError::ParseError`] if sysfs topology data is malformed.
/// Returns [`CpuAffinityError::NotSupported`] on non-Linux platforms.
#[cfg(target_os = "linux")]
pub fn set_cpu_affinity_physical(
    pid: Option<libc::pid_t>,
    cores: impl IntoIterator<Item = PhysicalCpuId>,
) -> Result<(), CpuAffinityError> {
    let cores: Vec<PhysicalCpuId> = cores.into_iter().collect();

    if cores.is_empty() {
        return Err(CpuAffinityError::EmptyCpuList);
    }

    let mapping = core_to_cpus_mapping()?;

    // Validate and collect the first CPU of each physical core in one pass
    let mut cpus = Vec::with_capacity(cores.len());
    for core in &cores {
        let core_cpus = mapping
            .get(core)
            .ok_or(CpuAffinityError::InvalidPhysicalCore {
                package_id: core.package_id,
                core_id: core.core_id,
            })?;
        // core_to_cpus_mapping guarantees non-empty, sorted vecs per core
        cpus.push(core_cpus[0]);
    }

    cpus.sort_unstable();
    set_cpu_affinity(pid, cpus)
}

#[cfg(not(target_os = "linux"))]
pub fn set_cpu_affinity_physical(
    _pid: Option<libc::pid_t>,
    _cores: impl IntoIterator<Item = PhysicalCpuId>,
) -> Result<(), CpuAffinityError> {
    Err(CpuAffinityError::NotSupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_physical_core_count() {
        match physical_core_count() {
            Ok(count) => {
                assert!(count > 0, "Should have at least one physical core");
                if let Ok(cpu_count) = crate::affinity::cpu_count() {
                    assert!(
                        count <= cpu_count,
                        "Physical cores ({count}) should not exceed total CPUs ({cpu_count})"
                    );
                }
            }
            Err(CpuAffinityError::NotSupported) => {}
            Err(e) => panic!("Unexpected error: {e:?}"),
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_core_to_cpus_mapping_consistency() {
        if let Ok(mapping) = core_to_cpus_mapping() {
            for (phys_id, cpus) in &mapping {
                assert!(
                    !cpus.is_empty(),
                    "Core {phys_id:?} should have at least one CPU"
                );

                let mut sorted = cpus.clone();
                sorted.sort_unstable();
                assert_eq!(cpus, &sorted, "CPUs for core {phys_id:?} should be sorted");
            }

            if let Ok(total_cpus) = crate::affinity::cpu_count() {
                let mapped_cpu_count: usize = mapping.values().map(|v| v.len()).sum();
                assert!(
                    mapped_cpu_count <= total_cpus,
                    "Mapped CPUs count ({mapped_cpu_count}) should not exceed total CPUs \
                     ({total_cpus})",
                );
            }
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_core_to_cpus_no_duplicates() {
        if let Ok(mapping) = core_to_cpus_mapping() {
            let mut all_cpus: Vec<CpuId> = Vec::new();
            for cpus in mapping.values() {
                all_cpus.extend(cpus);
            }

            let unique_count = {
                let mut seen = std::collections::HashSet::new();
                all_cpus.iter().filter(|cpu| seen.insert(cpu.0)).count()
            };
            assert_eq!(
                all_cpus.len(),
                unique_count,
                "Each CPU should belong to exactly one core"
            );
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_set_cpu_affinity_physical_validation() {
        assert!(matches!(
            set_cpu_affinity_physical(None, []).unwrap_err(),
            CpuAffinityError::EmptyCpuList
        ));

        let bogus = PhysicalCpuId {
            package_id: 99999,
            core_id: 99999,
        };
        assert!(matches!(
            set_cpu_affinity_physical(None, [bogus]).unwrap_err(),
            CpuAffinityError::InvalidPhysicalCore { .. }
        ));
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_not_supported_on_non_linux() {
        assert!(matches!(
            physical_core_count().unwrap_err(),
            CpuAffinityError::NotSupported
        ));
        assert!(matches!(
            core_to_cpus_mapping().unwrap_err(),
            CpuAffinityError::NotSupported
        ));
        let bogus = PhysicalCpuId {
            package_id: 0,
            core_id: 0,
        };
        assert!(matches!(
            set_cpu_affinity_physical(None, [bogus]).unwrap_err(),
            CpuAffinityError::NotSupported
        ));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_physical_vs_logical_consistency() {
        if let (Ok(physical), Ok(total)) = (physical_core_count(), crate::affinity::cpu_count()) {
            if physical > 0 {
                let threads_per_core = total.div_ceil(physical);
                assert!(
                    (1..=4).contains(&threads_per_core),
                    "Threads per core ({threads_per_core}) should be between 1 and 4"
                );
            }
        }
    }
}
