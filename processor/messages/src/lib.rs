use core::fmt;
use std::collections::HashMap;

use scale::{Encode, Decode};
use borsh::{BorshSerialize, BorshDeserialize};

use dkg::Participant;

use serai_primitives::BlockHash;
use validator_sets_primitives::{Session, KeyPair, Slash};
use coins_primitives::OutInstructionWithBalance;
use in_instructions_primitives::SignedBatch;

#[derive(Clone, Copy, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
pub struct SubstrateContext {
  pub serai_time: u64,
  pub network_latest_finalized_block: BlockHash,
}

pub mod key_gen {
  use super::*;

  #[derive(Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
  pub enum CoordinatorMessage {
    /// Instructs the Processor to begin the key generation process.
    ///
    /// This is sent by the Coordinator when it creates the Tributary (TODO).
    GenerateKey { session: Session, threshold: u16, evrf_public_keys: Vec<([u8; 32], Vec<u8>)> },
    /// Received participations for the specified key generation protocol.
    ///
    /// This is sent by the Coordinator's Tributary scanner.
    Participation { session: Session, participant: Participant, participation: Vec<u8> },
  }

  impl core::fmt::Debug for CoordinatorMessage {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
      match self {
        CoordinatorMessage::GenerateKey { session, threshold, evrf_public_keys } => fmt
          .debug_struct("CoordinatorMessage::GenerateKey")
          .field("session", &session)
          .field("threshold", &threshold)
          .field("evrf_public_keys.len()", &evrf_public_keys.len())
          .finish_non_exhaustive(),
        CoordinatorMessage::Participation { session, participant, .. } => fmt
          .debug_struct("CoordinatorMessage::Participation")
          .field("session", &session)
          .field("participant", &participant)
          .finish_non_exhaustive(),
      }
    }
  }

  #[derive(Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
  pub enum ProcessorMessage {
    // Participated in the specified key generation protocol.
    Participation { session: Session, participation: Vec<u8> },
    // Resulting keys from the specified key generation protocol.
    GeneratedKeyPair { session: Session, substrate_key: [u8; 32], network_key: Vec<u8> },
    // Blame this participant.
    Blame { session: Session, participant: Participant },
  }

  impl core::fmt::Debug for ProcessorMessage {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
      match self {
        ProcessorMessage::Participation { session, .. } => fmt
          .debug_struct("ProcessorMessage::Participation")
          .field("session", &session)
          .finish_non_exhaustive(),
        ProcessorMessage::GeneratedKeyPair { session, .. } => fmt
          .debug_struct("ProcessorMessage::GeneratedKeyPair")
          .field("session", &session)
          .finish_non_exhaustive(),
        ProcessorMessage::Blame { session, participant } => fmt
          .debug_struct("ProcessorMessage::Blame")
          .field("session", &session)
          .field("participant", &participant)
          .finish_non_exhaustive(),
      }
    }
  }
}

pub mod sign {
  use super::*;

  #[derive(Clone, Copy, PartialEq, Eq, Hash, Encode, Decode, BorshSerialize, BorshDeserialize)]
  pub enum VariantSignId {
    Cosign(u64),
    Batch([u8; 32]),
    SlashReport,
    Transaction([u8; 32]),
  }
  impl fmt::Debug for VariantSignId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
      match self {
        Self::Cosign(cosign) => {
          f.debug_struct("VariantSignId::Cosign").field("0", &cosign).finish()
        }
        Self::Batch(batch) => f.debug_struct("VariantSignId::Batch").field("0", &batch).finish(),
        Self::SlashReport => f.debug_struct("VariantSignId::SlashReport").finish(),
        Self::Transaction(tx) => {
          f.debug_struct("VariantSignId::Transaction").field("0", &hex::encode(tx)).finish()
        }
      }
    }
  }

  #[derive(
    Clone, Copy, PartialEq, Eq, Hash, Debug, Encode, Decode, BorshSerialize, BorshDeserialize,
  )]
  pub struct SignId {
    pub session: Session,
    pub id: VariantSignId,
    pub attempt: u32,
  }

  #[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
  pub enum CoordinatorMessage {
    /// Received preprocesses for the specified signing protocol.
    ///
    /// This is sent by the Coordinator's Tributary scanner.
    Preprocesses { id: SignId, preprocesses: HashMap<Participant, Vec<u8>> },
    // Received shares for the specified signing protocol.
    ///
    /// This is sent by the Coordinator's Tributary scanner.
    Shares { id: SignId, shares: HashMap<Participant, Vec<u8>> },
    // Re-attempt a signing protocol.
    ///
    /// This is sent by the Coordinator's Tributary re-attempt scheduling logic.
    Reattempt { id: SignId },
  }

  impl CoordinatorMessage {
    pub fn sign_id(&self) -> &SignId {
      match self {
        CoordinatorMessage::Preprocesses { id, .. } |
        CoordinatorMessage::Shares { id, .. } |
        CoordinatorMessage::Reattempt { id, .. } => id,
      }
    }
  }

  #[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
  pub enum ProcessorMessage {
    // Participant sent an invalid message during the sign protocol.
    InvalidParticipant { session: Session, participant: Participant },
    // Created preprocesses for the specified signing protocol.
    Preprocesses { id: SignId, preprocesses: Vec<Vec<u8>> },
    // Signed shares for the specified signing protocol.
    Shares { id: SignId, shares: Vec<Vec<u8>> },
  }
}

pub mod coordinator {
  use super::*;

  // TODO: Remove this for the one defined in serai-cosign
  pub fn cosign_block_msg(block_number: u64, block: [u8; 32]) -> Vec<u8> {
    const DST: &[u8] = b"Cosign";
    let mut res = vec![u8::try_from(DST.len()).unwrap()];
    res.extend(DST);
    res.extend(block_number.to_le_bytes());
    res.extend(block);
    res
  }

  #[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
  pub enum CoordinatorMessage {
    /// Cosign the specified Substrate block.
    ///
    /// This is sent by the Coordinator's Tributary scanner.
    CosignSubstrateBlock { session: Session, block_number: u64, block: [u8; 32] },
    /// Sign the slash report for this session.
    ///
    /// This is sent by the Coordinator's Tributary scanner.
    SignSlashReport { session: Session, report: Vec<Slash> },
  }

  #[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
  pub struct PlanMeta {
    pub session: Session,
    pub id: [u8; 32],
  }

  #[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
  pub enum ProcessorMessage {
    CosignedBlock { block_number: u64, block: [u8; 32], signature: Vec<u8> },
    SignedBatch { batch: SignedBatch },
    SubstrateBlockAck { block: u64, plans: Vec<PlanMeta> },
    SignedSlashReport { session: Session, signature: Vec<u8> },
  }
}

pub mod substrate {
  use super::*;

  #[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
  pub enum InInstructionResult {
    Succeeded,
    Failed,
  }
  #[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
  pub struct ExecutedBatch {
    pub id: u32,
    pub publisher: Session,
    pub external_network_block_hash: [u8; 32],
    pub in_instructions_hash: [u8; 32],
    pub in_instruction_results: Vec<InInstructionResult>,
  }

  #[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
  pub enum CoordinatorMessage {
    /// Keys set on the Serai blockchain.
    ///
    /// This is set by the Coordinator's Substrate canonical event stream.
    SetKeys { serai_time: u64, session: Session, key_pair: KeyPair },
    /// Slashes reported on the Serai blockchain OR the process timed out.
    ///
    /// This is the final message for a session,
    ///
    /// This is set by the Coordinator's Substrate canonical event stream.
    SlashesReported { session: Session },
    /// A block from Serai with relevance to this processor.
    ///
    /// This is set by the Coordinator's Substrate canonical event stream.
    Block {
      serai_block_number: u64,
      batch: Option<ExecutedBatch>,
      burns: Vec<OutInstructionWithBalance>,
    },
  }

  #[derive(Clone, PartialEq, Eq, Debug)]
  pub enum ProcessorMessage {}
  impl BorshSerialize for ProcessorMessage {
    fn serialize<W: borsh::io::Write>(&self, _writer: &mut W) -> borsh::io::Result<()> {
      unimplemented!()
    }
  }
  impl BorshDeserialize for ProcessorMessage {
    fn deserialize_reader<R: borsh::io::Read>(_reader: &mut R) -> borsh::io::Result<Self> {
      unimplemented!()
    }
  }
}

macro_rules! impl_from {
  ($from: ident, $to: ident, $via: ident) => {
    impl From<$from::$to> for $to {
      fn from(msg: $from::$to) -> $to {
        $to::$via(msg)
      }
    }
  };
}

#[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
pub enum CoordinatorMessage {
  KeyGen(key_gen::CoordinatorMessage),
  Sign(sign::CoordinatorMessage),
  Coordinator(coordinator::CoordinatorMessage),
  Substrate(substrate::CoordinatorMessage),
}

impl_from!(key_gen, CoordinatorMessage, KeyGen);
impl_from!(sign, CoordinatorMessage, Sign);
impl_from!(coordinator, CoordinatorMessage, Coordinator);
impl_from!(substrate, CoordinatorMessage, Substrate);

#[derive(Clone, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize)]
pub enum ProcessorMessage {
  KeyGen(key_gen::ProcessorMessage),
  Sign(sign::ProcessorMessage),
  Coordinator(coordinator::ProcessorMessage),
  Substrate(substrate::ProcessorMessage),
}

impl_from!(key_gen, ProcessorMessage, KeyGen);
impl_from!(sign, ProcessorMessage, Sign);
impl_from!(coordinator, ProcessorMessage, Coordinator);
impl_from!(substrate, ProcessorMessage, Substrate);

// Intent generation code

const COORDINATOR_UID: u8 = 0;
const PROCESSOR_UID: u8 = 1;

const TYPE_KEY_GEN_UID: u8 = 0;
const TYPE_SIGN_UID: u8 = 1;
const TYPE_COORDINATOR_UID: u8 = 2;
const TYPE_SUBSTRATE_UID: u8 = 3;

impl CoordinatorMessage {
  /// The intent for this message, which should be unique across the validator's entire system,
  /// including all of its processors.
  ///
  /// This doesn't use H(msg.serialize()) as it's meant to be unique to intent, not unique to
  /// values. While the values should be consistent per intent, that assumption isn't required
  /// here.
  pub fn intent(&self) -> Vec<u8> {
    match self {
      CoordinatorMessage::KeyGen(msg) => {
        let (sub, id) = match msg {
          // Unique since we only have one attempt per session
          key_gen::CoordinatorMessage::GenerateKey { session, .. } => {
            (0, borsh::to_vec(session).unwrap())
          }
          // Unique since one participation per participant per session
          key_gen::CoordinatorMessage::Participation { session, participant, .. } => {
            (1, borsh::to_vec(&(session, participant)).unwrap())
          }
        };

        let mut res = vec![COORDINATOR_UID, TYPE_KEY_GEN_UID, sub];
        res.extend(&id);
        res
      }
      CoordinatorMessage::Sign(msg) => {
        let (sub, id) = match msg {
          // Unique since SignId
          sign::CoordinatorMessage::Preprocesses { id, .. } => (0, id),
          sign::CoordinatorMessage::Shares { id, .. } => (1, id),
          sign::CoordinatorMessage::Reattempt { id, .. } => (2, id),
        };

        let mut res = vec![COORDINATOR_UID, TYPE_SIGN_UID, sub];
        res.extend(id.encode());
        res
      }
      CoordinatorMessage::Coordinator(msg) => {
        let (sub, id) = match msg {
          // We only cosign a block once, and Reattempt is a separate message
          coordinator::CoordinatorMessage::CosignSubstrateBlock { block_number, .. } => {
            (0, block_number.encode())
          }
          // We only sign one slash report, and Reattempt is a separate message
          coordinator::CoordinatorMessage::SignSlashReport { session, .. } => (1, session.encode()),
        };

        let mut res = vec![COORDINATOR_UID, TYPE_COORDINATOR_UID, sub];
        res.extend(&id);
        res
      }
      CoordinatorMessage::Substrate(msg) => {
        let (sub, id) = match msg {
          substrate::CoordinatorMessage::SetKeys { session, .. } => (0, session.encode()),
          substrate::CoordinatorMessage::SlashesReported { session } => (1, session.encode()),
          substrate::CoordinatorMessage::Block { serai_block_number, .. } => {
            (2, serai_block_number.encode())
          }
        };

        let mut res = vec![COORDINATOR_UID, TYPE_SUBSTRATE_UID, sub];
        res.extend(&id);
        res
      }
    }
  }
}

impl ProcessorMessage {
  /// The intent for this message, which should be unique across the validator's entire system,
  /// including all of its processors.
  ///
  /// This doesn't use H(msg.serialize()) as it's meant to be unique to intent, not unique to
  /// values. While the values should be consistent per intent, that assumption isn't required
  /// here.
  pub fn intent(&self) -> Vec<u8> {
    match self {
      ProcessorMessage::KeyGen(msg) => {
        let (sub, id) = match msg {
          // Unique since we only have one participation per session (due to no re-attempts)
          key_gen::ProcessorMessage::Participation { session, .. } => {
            (0, borsh::to_vec(session).unwrap())
          }
          key_gen::ProcessorMessage::GeneratedKeyPair { session, .. } => {
            (1, borsh::to_vec(session).unwrap())
          }
          // Unique since we only blame a participant once (as this is fatal)
          key_gen::ProcessorMessage::Blame { session, participant } => {
            (2, borsh::to_vec(&(session, participant)).unwrap())
          }
        };

        let mut res = vec![PROCESSOR_UID, TYPE_KEY_GEN_UID, sub];
        res.extend(&id);
        res
      }
      ProcessorMessage::Sign(msg) => {
        let (sub, id) = match msg {
          // Unique since we'll only fatally slash a a participant once
          sign::ProcessorMessage::InvalidParticipant { session, participant } => {
            (0, (session, u16::from(*participant)).encode())
          }
          // Unique since SignId
          sign::ProcessorMessage::Preprocesses { id, .. } => (1, id.encode()),
          sign::ProcessorMessage::Shares { id, .. } => (2, id.encode()),
        };

        let mut res = vec![PROCESSOR_UID, TYPE_SIGN_UID, sub];
        res.extend(&id);
        res
      }
      ProcessorMessage::Coordinator(msg) => {
        let (sub, id) = match msg {
          coordinator::ProcessorMessage::CosignedBlock { block, .. } => (0, block.encode()),
          coordinator::ProcessorMessage::SignedBatch { batch, .. } => (1, batch.batch.id.encode()),
          coordinator::ProcessorMessage::SubstrateBlockAck { block, .. } => (2, block.encode()),
          coordinator::ProcessorMessage::SignedSlashReport { session, .. } => (3, session.encode()),
        };

        let mut res = vec![PROCESSOR_UID, TYPE_COORDINATOR_UID, sub];
        res.extend(&id);
        res
      }
      ProcessorMessage::Substrate(_) => panic!("requesting intent for empty message type"),
    }
  }
}
