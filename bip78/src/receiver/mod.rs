
use bitcoin::{Script, TxOut, Address, Amount, Transaction, OutPoint};

mod error;

pub use error::RequestError;
use error::InternalRequestError;
use crate::psbt::{InputPair, Psbt};
use bitcoin::util::psbt::PartiallySignedTransaction;
use std::convert::TryFrom;

pub trait Headers {
    fn get_header(&self, key: &str) -> Option<&str>;
}


#[derive(Debug)]
pub struct UncheckedProposal {
    psbt: Psbt,
}

#[cfg(not(feature = "async"))]
/// All checks should return false to pass
// TODO return [Result]s
pub trait Checks {
    fn unbroacastable(&self, tx: &Transaction) -> bool;
    fn already_seen(&mut self, out_point: &OutPoint) -> bool;
    fn owned(&self, script_pubkey: &Script) -> bool;
}

#[cfg(feature = "async")]
/// All checks should return false to pass
// TODO return [Result]s
pub trait Checks {
    fn unbroacastable(&self, tx: &Transaction) -> bool;
    fn already_seen(&mut self, out_point: &OutPoint) -> bool;
    fn owned(&self, script_pubkey: &Script) -> bool;
}

#[derive(Debug)]
pub enum ChecksError {
    TxUnbroadcastable,
    TxinAlreadySeen,
    TxinOwned,
    MissingPrevout,
}

impl UncheckedProposal {
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
        let psbt = PartiallySignedTransaction::consensus_decode(reader).map_err(InternalRequestError::Decode)?;

        Ok(UncheckedProposal {
            psbt: Psbt::try_from(psbt).expect("deserialization ensure input/output counts"),
        })
    }

    #[cfg(feature = "async")]
    pub async fn check<C: Checks>(self, checks: &mut C) -> Result<Proposal, ChecksError> {
        let tx = self.psbt.clone().extract_tx();
        if checks.unbroacastable(&tx) {
            return Err(ChecksError::TxUnbroadcastable);
        }

        for input_pair in self.psbt.input_pairs() {
            if checks.owned(&input_pair.previous_txout().map_err(|_| ChecksError::MissingPrevout)?.script_pubkey) {
                return Err(ChecksError::TxinOwned);
            }

            if checks.already_seen(&input_pair.txin.previous_output) {
                return Err(ChecksError::TxinAlreadySeen);
            }
        }

        Ok(Proposal {
            psbt: self.psbt,
        })
    }

    #[cfg(not(feature = "async"))]
    pub fn check<C: Checks>(self, checks: &mut C) -> Result<Proposal, ChecksError> {
        let tx = self.psbt.clone().extract_tx();
        if checks.unbroacastable(&tx) {
            return Err(ChecksError::TxUnbroadcastable);
        }

        for input_pair in self.psbt.input_pairs() {
            if checks.owned(&input_pair.previous_txout().map_err(|_| ChecksError::MissingPrevout)?.script_pubkey) {
                return Err(ChecksError::TxinOwned);
            }

            if checks.already_seen(&input_pair.txin.previous_output) {
                return Err(ChecksError::TxinAlreadySeen);
            }
        }

        Ok(Proposal {
            psbt: self.psbt,
        })
    }
}


/// Transaction that must be broadcasted.
#[must_use = "The transaction must be broadcasted to prevent abuse"]
pub struct MustBroadcast(pub bitcoin::Transaction);

#[derive(Debug)]
pub struct Proposal {
    psbt: Psbt,
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

pub fn create_uri(address: &Address, amount: &Amount, pj: &str) -> String {
    format!("{}?amount={}&pj={}", address.to_qr_uri(), amount.as_btc(), pj)
}
