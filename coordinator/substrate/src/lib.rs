#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

use scale::{Encode, Decode};
use borsh::{io, BorshSerialize, BorshDeserialize};

use serai_client::{
  primitives::{NetworkId, PublicKey, Signature, SeraiAddress},
  validator_sets::primitives::{Session, ValidatorSet, KeyPair},
  in_instructions::primitives::SignedBatch,
  Transaction,
};

use serai_db::*;

mod canonical;
pub use canonical::CanonicalEventStream;
mod ephemeral;
pub use ephemeral::EphemeralEventStream;

mod set_keys;
pub use set_keys::SetKeysTask;
mod publish_batch;
pub use publish_batch::PublishBatchTask;
mod publish_slash_report;
pub use publish_slash_report::PublishSlashReportTask;

fn borsh_serialize_validators<W: io::Write>(
  validators: &Vec<(PublicKey, u16)>,
  writer: &mut W,
) -> Result<(), io::Error> {
  // This doesn't use `encode_to` as `encode_to` panics if the writer returns an error
  writer.write_all(&validators.encode())
}

fn borsh_deserialize_validators<R: io::Read>(
  reader: &mut R,
) -> Result<Vec<(PublicKey, u16)>, io::Error> {
  Decode::decode(&mut scale::IoReader(reader)).map_err(io::Error::other)
}

/// The information for a new set.
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct NewSetInformation {
  /// The set.
  pub set: ValidatorSet,
  /// The Serai block which declared it.
  pub serai_block: [u8; 32],
  /// The time of the block which declared it, in seconds.
  pub declaration_time: u64,
  /// The threshold to use.
  pub threshold: u16,
  /// The validators, with the amount of key shares they have.
  #[borsh(
    serialize_with = "borsh_serialize_validators",
    deserialize_with = "borsh_deserialize_validators"
  )]
  pub validators: Vec<(PublicKey, u16)>,
  /// The eVRF public keys.
  pub evrf_public_keys: Vec<([u8; 32], Vec<u8>)>,
}

mod _public_db {
  use super::*;

  db_channel!(
    CoordinatorSubstrate {
      // Canonical messages to send to the processor
      Canonical: (network: NetworkId) -> messages::substrate::CoordinatorMessage,

      // Relevant new set, from an ephemeral event stream
      NewSet: () -> NewSetInformation,
      // Potentially relevant sign slash report, from an ephemeral event stream
      SignSlashReport: (set: ValidatorSet) -> (),

      // Signed batches to publish onto the Serai network
      SignedBatches: (network: NetworkId) -> SignedBatch,
    }
  );

  create_db!(
    CoordinatorSubstrate {
      // Keys to set on the Serai network
      Keys: (network: NetworkId) -> (Session, Vec<u8>),
      // Slash reports to publish onto the Serai network
      SlashReports: (network: NetworkId) -> (Session, Vec<u8>),
    }
  );
}

/// The canonical event stream.
pub struct Canonical;
impl Canonical {
  pub(crate) fn send(
    txn: &mut impl DbTxn,
    network: NetworkId,
    msg: &messages::substrate::CoordinatorMessage,
  ) {
    _public_db::Canonical::send(txn, network, msg);
  }
  /// Try to receive a canonical event, returning `None` if there is none to receive.
  pub fn try_recv(
    txn: &mut impl DbTxn,
    network: NetworkId,
  ) -> Option<messages::substrate::CoordinatorMessage> {
    _public_db::Canonical::try_recv(txn, network)
  }
}

/// The channel for new set events emitted by an ephemeral event stream.
pub struct NewSet;
impl NewSet {
  pub(crate) fn send(txn: &mut impl DbTxn, msg: &NewSetInformation) {
    _public_db::NewSet::send(txn, msg);
  }
  /// Try to receive a new set's information, returning `None` if there is none to receive.
  pub fn try_recv(txn: &mut impl DbTxn) -> Option<NewSetInformation> {
    _public_db::NewSet::try_recv(txn)
  }
}

/// The channel for notifications to sign a slash report, as emitted by an ephemeral event stream.
///
/// These notifications MAY be for irrelevant validator sets. The only guarantee is the
/// notifications for all relevant validator sets will be included.
pub struct SignSlashReport;
impl SignSlashReport {
  pub(crate) fn send(txn: &mut impl DbTxn, set: ValidatorSet) {
    _public_db::SignSlashReport::send(txn, set, &());
  }
  /// Try to receive a notification to sign a slash report, returning `None` if there is none to
  /// receive.
  pub fn try_recv(txn: &mut impl DbTxn, set: ValidatorSet) -> Option<()> {
    _public_db::SignSlashReport::try_recv(txn, set)
  }
}

/// The keys to set on Serai.
pub struct Keys;
impl Keys {
  /// Set the keys to report for a validator set.
  ///
  /// This only saves the most recent keys as only a single session is eligible to have its keys
  /// reported at once.
  pub fn set(
    txn: &mut impl DbTxn,
    set: ValidatorSet,
    key_pair: KeyPair,
    signature_participants: bitvec::vec::BitVec<u8, bitvec::order::Lsb0>,
    signature: Signature,
  ) {
    // If we have a more recent pair of keys, don't write this historic one
    if let Some((existing_session, _)) = _public_db::Keys::get(txn, set.network) {
      if existing_session.0 >= set.session.0 {
        return;
      }
    }

    let tx = serai_client::validator_sets::SeraiValidatorSets::set_keys(
      set.network,
      key_pair,
      signature_participants,
      signature,
    );
    _public_db::Keys::set(txn, set.network, &(set.session, tx.encode()));
  }
  pub(crate) fn take(txn: &mut impl DbTxn, network: NetworkId) -> Option<(Session, Transaction)> {
    let (session, tx) = _public_db::Keys::take(txn, network)?;
    Some((session, <_>::decode(&mut tx.as_slice()).unwrap()))
  }
}

/// The signed batches to publish onto Serai.
pub struct SignedBatches;
impl SignedBatches {
  /// Send a `SignedBatch` to publish onto Serai.
  pub fn send(txn: &mut impl DbTxn, batch: &SignedBatch) {
    _public_db::SignedBatches::send(txn, batch.batch.network, batch);
  }
  pub(crate) fn try_recv(txn: &mut impl DbTxn, network: NetworkId) -> Option<SignedBatch> {
    _public_db::SignedBatches::try_recv(txn, network)
  }
}

/// The slash report was invalid.
#[derive(Debug)]
pub struct InvalidSlashReport;

/// The slash reports to publish onto Serai.
pub struct SlashReports;
impl SlashReports {
  /// Set the slashes to report for a validator set.
  ///
  /// This only saves the most recent slashes as only a single session is eligible to have its
  /// slashes reported at once.
  ///
  /// Returns Err if the slashes are invalid. Returns Ok if the slashes weren't detected as
  /// invalid. Slashes may be considered invalid by the Serai blockchain later even if not detected
  /// as invalid here.
  pub fn set(
    txn: &mut impl DbTxn,
    set: ValidatorSet,
    slashes: Vec<(SeraiAddress, u32)>,
    signature: Signature,
  ) -> Result<(), InvalidSlashReport> {
    // If we have a more recent slash report, don't write this historic one
    if let Some((existing_session, _)) = _public_db::SlashReports::get(txn, set.network) {
      if existing_session.0 >= set.session.0 {
        return Ok(());
      }
    }

    let tx = serai_client::validator_sets::SeraiValidatorSets::report_slashes(
      set.network,
      slashes.try_into().map_err(|_| InvalidSlashReport)?,
      signature,
    );
    _public_db::SlashReports::set(txn, set.network, &(set.session, tx.encode()));
    Ok(())
  }
  pub(crate) fn take(txn: &mut impl DbTxn, network: NetworkId) -> Option<(Session, Transaction)> {
    let (session, tx) = _public_db::SlashReports::take(txn, network)?;
    Some((session, <_>::decode(&mut tx.as_slice()).unwrap()))
  }
}
