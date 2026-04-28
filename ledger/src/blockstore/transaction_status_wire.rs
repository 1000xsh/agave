use {
    solana_message::v0::LoadedAddresses,
    solana_pubkey::Pubkey,
    solana_transaction_error::{TransactionError, TransactionResult as TxResult},
    std::convert::TryFrom,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum TransactionStatusMetaWireError {
    Malformed(&'static str),
    InvalidStatusError,
    InvalidLoadedWritableAddress,
    InvalidLoadedReadonlyAddress,
}

enum ParsedTransactionError {
    Valid(TransactionError),
    Invalid,
}

#[inline]
const fn malformed(reason: &'static str) -> TransactionStatusMetaWireError {
    TransactionStatusMetaWireError::Malformed(reason)
}

#[inline]
fn read_varint(data: &[u8]) -> std::result::Result<(u64, &[u8]), TransactionStatusMetaWireError> {
    let mut value = 0u64;
    let mut shift = 0u32;

    for (index, &byte) in data.iter().enumerate() {
        if shift >= 64 {
            return Err(malformed("varint too long"));
        }
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, &data[index + 1..]));
        }
        shift += 7;
    }

    Err(malformed("truncated varint"))
}

#[inline]
fn read_tag(data: &[u8]) -> std::result::Result<(u32, u8, &[u8]), TransactionStatusMetaWireError> {
    let (tag, rest) = read_varint(data)?;
    let field_number = (tag >> 3) as u32;
    if field_number == 0 {
        return Err(malformed("invalid field number"));
    }
    let wire_type = (tag & 0x07) as u8;
    Ok((field_number, wire_type, rest))
}

#[inline]
fn read_len_delimited(
    data: &[u8],
) -> std::result::Result<(&[u8], &[u8]), TransactionStatusMetaWireError> {
    let (len, rest) = read_varint(data)?;
    let len = usize::try_from(len).map_err(|_| malformed("length overflow"))?;
    if rest.len() < len {
        return Err(malformed("truncated length-delimited field"));
    }
    Ok((&rest[..len], &rest[len..]))
}

#[inline]
fn skip_field(
    wire_type: u8,
    data: &[u8],
) -> std::result::Result<&[u8], TransactionStatusMetaWireError> {
    match wire_type {
        0 => read_varint(data).map(|(_, rest)| rest),
        1 => data
            .get(8..)
            .ok_or_else(|| malformed("truncated 64-bit field")),
        2 => read_len_delimited(data).map(|(_, rest)| rest),
        5 => data
            .get(4..)
            .ok_or_else(|| malformed("truncated 32-bit field")),
        _ => Err(malformed("unsupported wire type")),
    }
}

fn parse_transaction_error_message(
    mut data: &[u8],
) -> std::result::Result<ParsedTransactionError, TransactionStatusMetaWireError> {
    let mut err_bytes = None;

    while !data.is_empty() {
        let (field_number, wire_type, rest) = read_tag(data)?;
        data = rest;
        match field_number {
            1 => {
                if wire_type != 2 {
                    return Err(malformed("transaction error field has invalid wire type"));
                }
                let (bytes, rest) = read_len_delimited(data)?;
                err_bytes = Some(bytes);
                data = rest;
            }
            _ => data = skip_field(wire_type, data)?,
        }
    }

    let Some(err_bytes) = err_bytes else {
        return Ok(ParsedTransactionError::Invalid);
    };

    Ok(match bincode::deserialize(err_bytes) {
        Ok(err) => ParsedTransactionError::Valid(err),
        Err(_) => ParsedTransactionError::Invalid,
    })
}

pub(super) fn parse_status(
    mut data: &[u8],
) -> std::result::Result<TxResult<()>, TransactionStatusMetaWireError> {
    let mut parsed_error = None;

    while !data.is_empty() {
        let (field_number, wire_type, rest) = read_tag(data)?;
        data = rest;
        match field_number {
            1 => {
                if wire_type != 2 {
                    return Err(malformed(
                        "transaction status err field has invalid wire type",
                    ));
                }
                let (message, rest) = read_len_delimited(data)?;
                parsed_error = Some(parse_transaction_error_message(message)?);
                data = rest;
            }
            _ => data = skip_field(wire_type, data)?,
        }
    }

    match parsed_error {
        None => Ok(Ok(())),
        Some(ParsedTransactionError::Valid(err)) => Ok(Err(err)),
        Some(ParsedTransactionError::Invalid) => {
            Err(TransactionStatusMetaWireError::InvalidStatusError)
        }
    }
}

pub(super) fn parse_loaded_addresses(
    mut data: &[u8],
) -> std::result::Result<LoadedAddresses, TransactionStatusMetaWireError> {
    let mut loaded_addresses = LoadedAddresses::default();

    while !data.is_empty() {
        let (field_number, wire_type, rest) = read_tag(data)?;
        data = rest;
        match field_number {
            12 | 13 => {
                if wire_type != 2 {
                    return Err(malformed("loaded addresses field has invalid wire type"));
                }
                let (bytes, rest) = read_len_delimited(data)?;
                let address = Pubkey::try_from(bytes).map_err(|_| match field_number {
                    12 => TransactionStatusMetaWireError::InvalidLoadedWritableAddress,
                    _ => TransactionStatusMetaWireError::InvalidLoadedReadonlyAddress,
                })?;
                if field_number == 12 {
                    loaded_addresses.writable.push(address);
                } else {
                    loaded_addresses.readonly.push(address);
                }
                data = rest;
            }
            _ => data = skip_field(wire_type, data)?,
        }
    }

    Ok(loaded_addresses)
}

#[cfg(test)]
mod tests {
    use {
        super::*, prost::Message, solana_message::v0::LoadedAddresses, solana_pubkey::Pubkey,
        solana_storage_proto::convert::generated, solana_transaction_error::TransactionError,
        solana_transaction_status::TransactionStatusMeta,
    };

    #[test]
    fn test_parse_status_success() {
        let encoded = generated::TransactionStatusMeta::from(TransactionStatusMeta::default())
            .encode_to_vec();
        assert_eq!(parse_status(&encoded).unwrap(), Ok(()));
    }

    #[test]
    fn test_parse_status_error() {
        let encoded = generated::TransactionStatusMeta::from(TransactionStatusMeta {
            status: Err(TransactionError::InsufficientFundsForFee),
            ..TransactionStatusMeta::default()
        })
        .encode_to_vec();

        assert_eq!(
            parse_status(&encoded).unwrap(),
            Err(TransactionError::InsufficientFundsForFee)
        );
    }

    #[test]
    fn test_parse_status_invalid_error_payload() {
        let encoded = generated::TransactionStatusMeta {
            err: Some(generated::TransactionError { err: vec![] }),
            ..generated::TransactionStatusMeta::default()
        }
        .encode_to_vec();

        assert_eq!(
            parse_status(&encoded),
            Err(TransactionStatusMetaWireError::InvalidStatusError)
        );
    }

    #[test]
    fn test_parse_loaded_addresses() {
        let expected = LoadedAddresses {
            writable: vec![Pubkey::new_unique()],
            readonly: vec![Pubkey::new_unique()],
        };
        let encoded = generated::TransactionStatusMeta::from(TransactionStatusMeta {
            loaded_addresses: expected.clone(),
            ..TransactionStatusMeta::default()
        })
        .encode_to_vec();

        assert_eq!(parse_loaded_addresses(&encoded).unwrap(), expected);
    }
}
