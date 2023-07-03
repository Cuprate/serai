use serai_db::{DbTxn, Db};

use crate::messages::*;

#[derive(Clone, Debug)]
pub(crate) struct Queue<D: Db>(pub(crate) D, pub(crate) Service);
impl<D: Db> Queue<D> {
  fn key(domain: &'static [u8], key: impl AsRef<[u8]>) -> Vec<u8> {
    [&[u8::try_from(domain.len()).unwrap()], domain, key.as_ref()].concat()
  }

  fn message_count_key(&self) -> Vec<u8> {
    Self::key(b"message_count", serde_json::to_vec(&self.1).unwrap())
  }
  pub(crate) fn message_count(&self) -> u64 {
    self
      .0
      .get(self.message_count_key())
      .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
      .unwrap_or(0)
  }

  fn last_acknowledged_key(&self) -> Vec<u8> {
    Self::key(b"last_acknowledged", serde_json::to_vec(&self.1).unwrap())
  }
  pub(crate) fn last_acknowledged(&self) -> Option<u64> {
    self
      .0
      .get(self.last_acknowledged_key())
      .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
  }

  fn message_key(&self, id: u64) -> Vec<u8> {
    Self::key(b"message", serde_json::to_vec(&(self.1, id)).unwrap())
  }
  pub(crate) fn queue_message(&mut self, msg: QueuedMessage) {
    let id = self.message_count();
    let msg_key = self.message_key(id);
    let msg_count_key = self.message_count_key();

    let mut txn = self.0.txn();
    txn.put(msg_key, serde_json::to_vec(&msg).unwrap());
    txn.put(msg_count_key, (id + 1).to_le_bytes());
    txn.commit();
  }

  pub(crate) fn get_message(&self, id: u64) -> Option<QueuedMessage> {
    self.0.get(self.message_key(id)).map(|bytes| serde_json::from_slice(&bytes).unwrap())
  }

  pub(crate) fn ack_message(&mut self, id: u64) {
    let ack_key = self.last_acknowledged_key();
    let mut txn = self.0.txn();
    txn.put(ack_key, id.to_le_bytes());
    txn.commit();
  }
}
