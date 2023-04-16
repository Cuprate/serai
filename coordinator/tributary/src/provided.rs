use std::collections::{VecDeque, HashMap};

use thiserror::Error;

use serai_db::{Get, DbTxn, Db};

use crate::{TransactionKind, TransactionError, Transaction, verify_transaction};

#[derive(Clone, PartialEq, Eq, Debug, Error)]
pub enum ProvidedError {
  /// The provided transaction's kind wasn't Provided
  #[error("transaction wasn't a provided transaction")]
  NotProvided,
  /// The provided transaction was invalid.
  #[error("provided transaction was invalid")]
  InvalidProvided(TransactionError),
  /// Transaction was already provided
  #[error("transaction was already provided")]
  AlreadyProvided,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProvidedTransactions<D: Db, T: Transaction> {
  db: D,
  genesis: [u8; 32],

  pub(crate) transactions: VecDeque<T>,
}

impl<D: Db, T: Transaction> ProvidedTransactions<D, T> {
  fn transaction_key(&self, hash: &[u8]) -> Vec<u8> {
    D::key(b"tributary_provided", b"transaction", [self.genesis.as_ref(), hash].concat())
  }
  fn current_provided_key(&self) -> Vec<u8> {
    D::key(b"tributary_provided", b"current", self.genesis)
  }

  pub(crate) fn new(db: D, genesis: [u8; 32]) -> Self {
    let mut res = ProvidedTransactions { db, genesis, transactions: VecDeque::new() };

    let currently_provided = res.db.get(res.current_provided_key()).unwrap_or(vec![]);
    let mut i = 0;
    while i < currently_provided.len() {
      res.transactions.push_back(
        T::read::<&[u8]>(
          &mut res
            .db
            .get(res.transaction_key(&currently_provided[i .. (i + 32)]))
            .unwrap()
            .as_ref(),
        )
        .unwrap(),
      );
      i += 32;
    }

    res
  }

  /// Provide a transaction for inclusion in a block.
  pub(crate) fn provide(&mut self, tx: T) -> Result<(), ProvidedError> {
    if tx.kind() != TransactionKind::Provided {
      Err(ProvidedError::NotProvided)?;
    }

    match verify_transaction(&tx, self.genesis, &mut HashMap::new()) {
      Ok(()) => {}
      Err(e) => Err(ProvidedError::InvalidProvided(e))?,
    }

    let tx_hash = tx.hash();
    let provided_key = self.transaction_key(&tx_hash);
    if self.db.get(&provided_key).is_some() {
      Err(ProvidedError::AlreadyProvided)?;
    }

    let current_provided_key = self.current_provided_key();
    let mut currently_provided = self.db.get(&current_provided_key).unwrap_or(vec![]);

    let mut txn = self.db.txn();
    txn.put(provided_key, tx.serialize());
    currently_provided.extend(tx_hash);
    txn.put(current_provided_key, currently_provided);
    txn.commit();

    self.transactions.push_back(tx);
    Ok(())
  }

  /// Complete a provided transaction, no longer proposing it nor voting for its validity.
  pub(crate) fn complete(&mut self, txn: &mut D::Transaction<'_>, tx: [u8; 32]) {
    assert_eq!(self.transactions.pop_front().unwrap().hash(), tx);

    let current_provided_key = self.current_provided_key();
    let mut currently_provided = txn.get(&current_provided_key).unwrap();
    assert_eq!(&currently_provided.drain(.. 32).collect::<Vec<_>>(), &tx);
    txn.put(current_provided_key, currently_provided);
  }
}