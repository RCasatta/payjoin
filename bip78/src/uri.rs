#[cfg(feature = "sender")]
use crate::sender;
use std::borrow::Cow;
use std::convert::TryFrom;

#[derive(Debug, Eq, PartialEq)]
pub struct Uri<'a> {
    pub(crate) address: bitcoin::Address,
    pub(crate) amount: bitcoin::Amount,
    pub(crate) endpoint: Cow<'a, str>,
    pub(crate) disable_output_substitution: bool,
}

impl<'a> Uri<'a> {
    pub fn address(&self) -> &bitcoin::Address {
        &self.address
    }

    pub fn amount(&self) -> bitcoin::Amount {
        self.amount
    }

    pub fn is_output_substitution_disabled(&self) -> bool {
        self.disable_output_substitution
    }

    #[cfg(feature = "sender")]
    pub fn create_request(
        self,
        psbt: bitcoin::util::psbt::PartiallySignedTransaction,
        params: sender::Params,
    ) -> Result<(sender::Request, sender::Context), sender::CreateRequestError> {
        sender::from_psbt_and_uri(psbt, self, params)
    }

    pub fn into_static(self) -> Uri<'static> {
        Uri {
            address: self.address,
            amount: self.amount,
            endpoint: Cow::Owned(self.endpoint.into()),
            disable_output_substitution: self.disable_output_substitution,
        }
    }
}

impl<'a> TryFrom<&'a str> for Uri<'a> {
    type Error = ParseUriError;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        fn match_kv<'a, T, E: Into<ParseUriError>, F: FnOnce(&'a str) -> Result<T, E>>(
            kv: &'a str,
            prefix: &'static str,
            out: &mut Option<T>,
            fun: F,
        ) -> Result<(), ParseUriError>
        where
            ParseUriError: From<E>,
        {
            if kv.starts_with(prefix) {
                let value = fun(&kv[prefix.len()..])?;
                if out.is_some() {
                    return Err(InternalBip21Error::DuplicateKey(prefix).into());
                }
                *out = Some(value);
            }
            Ok(())
        }


        let prefix = "bitcoin:";
        // from bip21: The scheme component ("bitcoin:") is case-insensitive, and implementations
        // must accept any combination of uppercase and lowercase letters. The rest of the URI is
        // case-sensitive, including the query parameter keys.
        if !s
            .chars()
            .zip(prefix.chars())
            .all(|(left, right)| left.to_ascii_lowercase() == right) || s.len()<8
        {
            return Err(InternalBip21Error::BadSchema(s.into()).into());
        }
        let uri_without_prefix = &s[prefix.len()..];
        let question_mark_pos = uri_without_prefix
            .find('?')
            .ok_or(ParseUriError::PjNotPresent)?;
        let address = uri_without_prefix[..question_mark_pos]
            .parse()
            .map_err(InternalBip21Error::Address)?;
        let mut amount = None;
        let mut endpoint = None;
        let mut disable_pjos = None;

        for kv in uri_without_prefix[(question_mark_pos + 1)..].split('&') {
            match_kv(kv, "amount=", &mut amount, |s| {
                bitcoin::Amount::from_str_in(s, bitcoin::Denomination::Bitcoin)
                    .map_err(InternalBip21Error::Amount)
            })?;
            match_kv(kv, "pjos=", &mut disable_pjos, |s| {
                if s == "0" {
                    Ok(true)
                } else if s == "1" {
                    Ok(false)
                } else {
                    Err(InternalPjParseError::BadPjos(s.into()))
                }
            })?;
            match_kv(kv, "pj=", &mut endpoint, |s| {
                if s.starts_with("https://") || s.starts_with("http://") {
                    Ok(s)
                } else {
                    Err(InternalPjParseError::BadSchema(s.into()))
                }
            })?;
        }

        match (amount, endpoint, disable_pjos) {
            (_, None, None) => Err(ParseUriError::PjNotPresent),
            (Some(amount), Some(endpoint), disable_pjos) => Ok(Uri { address, amount,
                endpoint: endpoint.into(),
                disable_output_substitution: disable_pjos.unwrap_or(false),
            }),
            (None, Some(_), _) => Err(ParseUriError::PayJoin(PjParseError(
                InternalPjParseError::MissingAmount,
            ))),
            (None, None, Some(_)) => Err(ParseUriError::PayJoin(PjParseError(
                InternalPjParseError::MissingAmountAndEndpoint,
            ))),
            (Some(_), None, Some(_)) => Err(ParseUriError::PayJoin(PjParseError(
                InternalPjParseError::MissingEndpoint,
            ))),
        }
    }
}

impl std::str::FromStr for Uri<'static> {
    type Err = ParseUriError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uri::try_from(s).map(Uri::into_static)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ParseUriError {
    PjNotPresent,
    Bip21(Bip21Error),
    PayJoin(PjParseError),
}

#[derive(Debug, Eq, PartialEq)]
pub struct Bip21Error(InternalBip21Error);

#[derive(Debug, Eq, PartialEq)]
pub struct PjParseError(InternalPjParseError);

#[derive(Debug, Eq, PartialEq)]
enum InternalBip21Error {
    Amount(bitcoin::util::amount::ParseAmountError),
    DuplicateKey(&'static str),
    BadSchema(String),
    Address(bitcoin::util::address::Error),
}

#[derive(Debug, Eq, PartialEq)]
enum InternalPjParseError {
    BadPjos(String),
    BadSchema(String),
    MissingAmount,
    MissingAmountAndEndpoint,
    MissingEndpoint,
}

impl From<Bip21Error> for ParseUriError {
    fn from(value: Bip21Error) -> Self {
        ParseUriError::Bip21(value)
    }
}

impl From<PjParseError> for ParseUriError {
    fn from(value: PjParseError) -> Self {
        ParseUriError::PayJoin(value)
    }
}

impl From<InternalBip21Error> for ParseUriError {
    fn from(value: InternalBip21Error) -> Self {
        Bip21Error(value).into()
    }
}

impl From<InternalPjParseError> for ParseUriError {
    fn from(value: InternalPjParseError) -> Self {
        PjParseError(value).into()
    }
}

#[cfg(test)]
mod tests {
    use crate::uri::{InternalBip21Error, InternalPjParseError};
    use crate::{Bip21Error, ParseUriError, Uri, PjParseError};
    use bitcoin::Address;
    use std::convert::TryFrom;
    use std::str::FromStr;
    use crate::bitcoin::util::amount::ParseAmountError;
    use crate::bitcoin::util;

    #[test]
    fn test_empty() {
        assert!(Uri::from_str("").is_err());
        assert!(Uri::from_str("bitcoin").is_err());
        assert!(Uri::from_str("bitcoin:").is_err());
    }

    #[test]
    fn test_valid() {
        for pj in ["https://example.com", "http://example.com", "http://vjdpwgybvubne5hda6v4c5iaeeevhge6jvo3w2cl6eocbwwvwxp7b7qd.onion"].iter() {
            let pj = format!("bitcoin:12c6DSiU4Rq3P4ZxziKxzrL5LmMBrzjrJX?amount=20.3&pj={}", pj);
            assert!(pj.parse::<Uri>().is_ok());
        }

        assert!("bitcoin:TB1Q6D3A2W975YNY0ASUVD9A67NER4NKS58FF0Q8G4?amount=0.0001&pj=https://testnet.demo.btcpayserver.org/BTC/pj".parse::<Uri>().is_ok());

        //TODO we may want endpoint to be a valid URL
        assert!(Uri::from_str("bitcoin:TB1Q6D3A2W975YNY0ASUVD9A67NER4NKS58FF0Q8G4?amount=1&pj=http://a").is_ok());

    }

    #[test]
    fn test_errors() {
        assert_eq!(
            Uri::from_str("bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W"),
            Err::<Uri<'_>, ParseUriError>(ParseUriError::PjNotPresent)
        );

        assert_eq!(
            Uri::from_str("bitcoinz:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W"),
            Err::<Uri<'_>, ParseUriError>(
                InternalBip21Error::BadSchema(
                    "bitcoinz:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W".to_string()
                )
                .into()
            )
        );

        assert_eq!(
            Uri::from_str("bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?amount=20.3&label=Luke-Jr"),
            Err::<Uri<'_>, ParseUriError>(
                InternalBip21Error::Address(util::address::Error::Base58(util::base58::Error::BadChecksum(291609738, 694262922))).into()
            )
        );

        assert_eq!(
            Uri::from_str("bitcoin:TB1Q6D3A2W975YNY0ASUVD9A67NER4NKS58FF0Q8G4?pj=https://testnet.demo.btcpayserver.org/BTC/pj"),
            Err::<Uri<'_>, ParseUriError>(InternalPjParseError::MissingAmount.into())
        );

        assert_eq!(
            Uri::from_str("bitcoin:TB1Q6D3A2W975YNY0ASUVD9A67NER4NKS58FF0Q8G4?pj=https://testnet.demo.btcpayserver.org/BTC/pj&amount="),
            Err::<Uri<'_>, ParseUriError>(InternalBip21Error::Amount(ParseAmountError::InvalidFormat).into())
        );

        assert_eq!(
            Uri::from_str("bitcoin:TB1Q6D3A2W975YNY0ASUVD9A67NER4NKS58FF0Q8G4?pj=https://testnet.demo.btcpayserver.org/BTC/pj&amount=1BTC"),
            Err::<Uri<'_>, ParseUriError>(InternalBip21Error::Amount(ParseAmountError::InvalidCharacter('B')).into())
        );

        assert_eq!(
            Uri::from_str("bitcoin:TB1Q6D3A2W975YNY0ASUVD9A67NER4NKS58FF0Q8G4?pj=https://testnet.demo.btcpayserver.org/BTC/pj&amount=999999999999999999999"),
            Err::<Uri<'_>, ParseUriError>(InternalBip21Error::Amount(ParseAmountError::TooBig).into())
        );
    }
}
