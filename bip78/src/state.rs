use bitcoin::util::psbt::PartiallySignedTransaction;
use std::marker::PhantomData;
use std::convert::{TryFrom, TryInto};
use crate::bitcoin::{Transaction, Script};
use crate::psbt::Psbt;
use bitcoin::OutPoint;

#[derive(Clone, Debug)]
pub struct PsbtState<S> {
    psbt: PartiallySignedTransaction,
    state: S
}

#[derive(Debug)]
pub enum PsbtError {
    UnequalInputCounts { tx_ins: usize, psbt_ins: usize, },
    UnequalOutputCounts { tx_outs: usize, psbt_outs: usize, },
    Todo,
}

pub trait Next<S> {
    fn next(self) -> PsbtState<S>;
}

pub trait TryNext<S> {
    fn try_next(self) -> Result<PsbtState<S>, PsbtError>;
}

/// Validate a [`PartiallySignedTransaction`] by checking transaction inputs number are equal to the
/// psbt inputs and the same for outputs. Note deserialization already have this guarantee (TODO verify)
#[derive(Clone, Debug)]
struct Validated;

///
#[derive(Clone, Debug, Default)]
struct MaybeUnbroadcastable {
    is_broadcastable: bool,
}

///
#[derive(Clone, Debug, Default)]
struct MaybeInputsOwned {
    are_previous_script_pubkey_not_mine: bool,
}

///
#[derive(Clone, Debug, Default)]
struct MaybePrevoutsSeen {
    are_prevouts_never_seen: bool,
}

///
#[derive(Clone, Debug)]
struct Proposal;

impl TryFrom<PartiallySignedTransaction> for PsbtState<Validated> {
    type Error = PsbtError;

    fn try_from(psbt: PartiallySignedTransaction) -> Result<Self, Self::Error> {
        let tx_ins = psbt.global.unsigned_tx.input.len();
        let psbt_ins = psbt.inputs.len();
        let tx_outs = psbt.global.unsigned_tx.output.len();
        let psbt_outs = psbt.outputs.len();

        if psbt_ins != tx_ins {
            Err(PsbtError::UnequalInputCounts { tx_ins, psbt_ins, })
        } else if psbt_outs != tx_outs {
            Err(PsbtError::UnequalOutputCounts { tx_outs, psbt_outs, })
        } else {
            Ok(PsbtState {
                psbt,
                state: Validated,
            })
        }
    }
}

impl From<PsbtState<Validated>> for PsbtState<MaybeUnbroadcastable> {
    fn from(psbt_state: PsbtState<Validated>) -> Self {
        PsbtState {
            psbt: psbt_state.psbt,
            state: Default::default(),
        }
    }
}

impl TryFrom<PsbtState<MaybeUnbroadcastable>> for PsbtState<MaybeInputsOwned> {
    type Error = PsbtError;

    fn try_from(value: PsbtState<MaybeUnbroadcastable>) -> Result<Self, Self::Error> {
        if value.state.is_broadcastable {
            Ok(PsbtState {
                psbt: value.psbt,
                state: Default::default(),
            })
        } else {
            Err(PsbtError::Todo)
        }
    }
}

impl TryFrom<PsbtState<MaybeInputsOwned>> for PsbtState<MaybePrevoutsSeen> {
    type Error = PsbtError;

    fn try_from(value: PsbtState<MaybeInputsOwned>) -> Result<Self, Self::Error> {
        if value.state.are_previous_script_pubkey_not_mine {
            Ok(PsbtState {
                psbt: value.psbt,
                state: Default::default(),
            })
        } else {
            Err(PsbtError::Todo)
        }
    }
}

impl TryFrom<PsbtState<MaybePrevoutsSeen>> for PsbtState<Proposal> {
    type Error = PsbtError;

    fn try_from(value: PsbtState<MaybePrevoutsSeen>) -> Result<Self, Self::Error> {
        if value.state.are_prevouts_never_seen {
            Ok(PsbtState {
                psbt: value.psbt,
                state: Proposal,
            })
        } else {
            Err(PsbtError::Todo)
        }
    }
}

impl TryNext<Validated> for PartiallySignedTransaction {
    fn try_next(self) -> Result<PsbtState<Validated>, PsbtError> {
        PsbtState::<Validated>::try_from(self)
    }
}


impl PsbtState<MaybeUnbroadcastable> {
    pub fn tx(&self) -> Transaction {
        self.psbt.clone().extract_tx()
    }
}

impl TryNext<MaybeInputsOwned> for PsbtState<MaybeUnbroadcastable> {
    fn try_next(self) -> Result<PsbtState<MaybeInputsOwned>, PsbtError> {
        PsbtState::<MaybeInputsOwned>::try_from(self)
    }
}

impl PsbtState<MaybeInputsOwned> {
    pub fn script_pubkeys(&self) -> impl Iterator<Item=&Script> + '_ {
        self.psbt.global.unsigned_tx.input.iter().map(|e| &e.script_sig)
    }
}

impl TryNext<MaybePrevoutsSeen> for PsbtState<MaybeInputsOwned> {
    fn try_next(self) -> Result<PsbtState<MaybePrevoutsSeen>, PsbtError> {
        PsbtState::<MaybePrevoutsSeen>::try_from(self)
    }
}

impl PsbtState<MaybePrevoutsSeen> {
    pub fn outpoints(&self) -> impl Iterator<Item=OutPoint> + '_ {
        self.psbt.global.unsigned_tx.input.iter().map(|e| e.previous_output)
    }
}

impl TryNext<Proposal> for PsbtState<MaybePrevoutsSeen> {
    fn try_next(self) -> Result<PsbtState<Proposal>, PsbtError> {
        PsbtState::<Proposal>::try_from(self)
    }
}

fn load_psbt_from_base64(mut input: impl std::io::Read) -> Result<PartiallySignedTransaction, bitcoin::consensus::encode::Error> {
    use bitcoin::consensus::Decodable;

    let reader = base64::read::DecoderReader::new(&mut input, base64::Config::new(base64::CharacterSet::Standard, true));
    PartiallySignedTransaction::consensus_decode(reader)
}

/*

digraph G {

  base64 -> PartiallySignedTransaction [label="deserialize"]
  PartiallySignedTransaction -> Validated [label="try_from"]
  Validated -> Original [label="from"]

  Validated -> MaybeUnbroadcastable [label="from"]
  MaybeUnbroadcastable -> MaybeInputsOwned [label="try_from"]
  MaybeInputsOwned -> MaybePrevoutsSeen [label="try_from"]
  MaybePrevoutsSeen -> Proposal [label="try_from"]

  Original [color=blue]

  MaybeUnbroadcastable [color=green]
  MaybeInputsOwned [color=green]
  MaybePrevoutsSeen [color=green]
  Proposal [color=green]

  Legend [shape=box,label="Legend:\n\ngreen: receiver flow\nblue: sender flow"]

}

 */

macro_rules! impl_from {
    ( $from_state:ty, $to_state:ty ) => {
        impl From<PsbtState<$from_state>> for PsbtState<$to_state> {
            fn from(psbt_state: PsbtState<$from_state>) -> Self {
                PsbtState {
                    psbt: psbt_state.psbt,
                    state: Default::default(),
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::state::{PsbtState, Validated, MaybeUnbroadcastable, Next, TryNext};
    use std::convert::{TryInto, TryFrom};

    #[test]
    fn test_state() {
        let mut original_psbt = "cHNidP8BAHMCAAAAAY8nutGgJdyYGXWiBEb45Hoe9lWGbkxh/6bNiOJdCDuDAAAAAAD+////AtyVuAUAAAAAF6kUHehJ8GnSdBUOOv6ujXLrWmsJRDCHgIQeAAAAAAAXqRR3QJbbz0hnQ8IvQ0fptGn+votneofTAAAAAAEBIKgb1wUAAAAAF6kU3k4ekGHKWRNbA1rV5tR5kEVDVNCHAQcXFgAUx4pFclNVgo1WWAdN1SYNX8tphTABCGsCRzBEAiB8Q+A6dep+Rz92vhy26lT0AjZn4PRLi8Bf9qoB/CMk0wIgP/Rj2PWZ3gEjUkTlhDRNAQ0gXwTO7t9n+V14pZ6oljUBIQMVmsAaoNWHVMS02LfTSe0e388LNitPa1UQZyOihY+FFgABABYAFEb2Giu6c4KO5YW0pfw3lGp9jMUUAAA=".as_bytes();

        let original_psbt = super::load_psbt_from_base64(&mut original_psbt).unwrap();
        let validated = original_psbt.try_next().unwrap();
        let mut maybe_broadcastable: PsbtState<MaybeUnbroadcastable> = validated.into();
        assert!(maybe_broadcastable.clone().try_next().is_err());

        let _ = maybe_broadcastable.tx(); // check is broadcastable
        maybe_broadcastable.state.is_broadcastable = true;

        let mut maybe_owned = maybe_broadcastable.try_next().unwrap();
        assert!(maybe_owned.clone().try_next().is_err());

        let _ = maybe_owned.script_pubkeys(); // check scripts aren't mine
        maybe_owned.state.are_previous_script_pubkey_not_mine = true;

        let mut maybe_seen = maybe_owned.try_next().unwrap();
        assert!(maybe_seen.clone().try_next().is_err());

        let _ = maybe_seen.outpoints(); // check outpoint aren't already seen
        maybe_seen.state.are_prevouts_never_seen = true;

        let _ = maybe_seen.try_next().unwrap();

    }
}