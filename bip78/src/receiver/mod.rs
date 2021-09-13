use bitcoin::util::psbt::PartiallySignedTransaction as Psbt;
use bitcoin::{Script, TxOut};
use std::convert::TryInto;

mod error;
pub mod state;

pub use error::RequestError;
use error::InternalRequestError;
use state::{PsbtState, Validated};

pub trait Headers {
    fn get_header(&self, key: &str) -> Option<&str>;
}

impl PsbtState<Validated> {
    pub fn from_request(body: impl std::io::Read, query: &str, headers: impl Headers) -> Result<Self, RequestError> {
        use crate::bitcoin::consensus::Decodable;

        let content_type = headers.get_header("content-type").ok_or(InternalRequestError::MissingHeader("Content-Type"))?;
        if content_type != "text/plain" {
            return Err(InternalRequestError::InvalidContentType(content_type.to_owned()).into());
        }
        let content_length = headers
            .get_header("content-length")
            .ok_or(InternalRequestError::MissingHeader("Content-Length"))?
            .parse::<u64>()
            .map_err(InternalRequestError::InvalidContentLength)?;
        // 4M block size limit with base64 encoding overhead => maximum reasonable size of content-length
        if content_length > 4_000_000 * 4 / 3 {
            return Err(InternalRequestError::ContentLengthTooLarge(content_length).into());
        }

        // enforce the limit
        let mut limited = body.take(content_length);
        let reader = base64::read::DecoderReader::new(&mut limited, base64::STANDARD);
        let psbt = Psbt::consensus_decode(reader).map_err(InternalRequestError::Decode)?;
        let validated : PsbtState<Validated> = psbt.try_into().expect("deserialization guarantee input/output number is correct");

        Ok(validated)
    }

}

/*
impl Proposal {
    pub fn replace_output_script(&mut self, new_output_script: Script, options: NewOutputOptions) -> Result<Self, OutputError> {
    }

    pub fn replace_output(&mut self, new_output: TxOut, options: NewOutputOptions) -> Result<Self, OutputError> {
    }

    pub fn insert_output(&mut self, new_output: TxOut, options: NewOutputOptions) -> Result<Self, OutputError> {
    }

    pub fn expected_missing_fee_for_replaced_output(&self, output_type: OutputType) -> bitcoin::Amount {
    }
}
*/

pub struct ReceiverOptions {
    dust_limit: bitcoin::Amount,
}

pub enum BumpFeePolicy {
    FailOnInsufficient,
    SubtractOurFeeOutput,
}

pub struct NewOutputOptions {
    set_as_fee_output: bool,
    subtract_fees_from_this: bool,
}

