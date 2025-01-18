use core::future::Future;

use ciphersuite::Ristretto;
use frost::dkg::ThresholdKeys;

use scale::Encode;
use serai_primitives::Signature;
use serai_validator_sets_primitives::Session;

use serai_db::{DbTxn, Db};

use serai_cosign::{COSIGN_CONTEXT, Cosign as CosignStruct, SignedCosign};
use messages::sign::VariantSignId;

use primitives::task::{DoesNotError, ContinuallyRan};

use frost_attempt_manager::*;

use crate::{
  db::{ToCosign, Cosign, CoordinatorToCosignerMessages, CosignerToCoordinatorMessages},
  WrappedSchnorrkelMachine,
};

mod db;
use db::LatestCosigned;

/// Fetches the latest cosign information and works on it.
///
/// Only the latest cosign attempt is kept. We don't work on historical attempts as later cosigns
/// supersede them.
#[allow(non_snake_case)]
pub(crate) struct CosignerTask<D: Db> {
  db: D,

  session: Session,
  keys: Vec<ThresholdKeys<Ristretto>>,

  current_cosign: Option<CosignStruct>,
  attempt_manager: AttemptManager<D, WrappedSchnorrkelMachine>,
}

impl<D: Db> CosignerTask<D> {
  pub(crate) fn new(db: D, session: Session, keys: Vec<ThresholdKeys<Ristretto>>) -> Self {
    let attempt_manager = AttemptManager::new(
      db.clone(),
      session,
      keys.first().expect("creating a cosigner with 0 keys").params().i(),
    );

    Self { db, session, keys, current_cosign: None, attempt_manager }
  }
}

impl<D: Db> ContinuallyRan for CosignerTask<D> {
  type Error = DoesNotError;

  fn run_iteration(&mut self) -> impl Send + Future<Output = Result<bool, DoesNotError>> {
    async move {
      let mut iterated = false;

      // Check the cosign to work on
      {
        let mut txn = self.db.txn();
        if let Some(cosign) = ToCosign::get(&txn, self.session) {
          // If this wasn't already signed for...
          if LatestCosigned::get(&txn, self.session) < Some(cosign.block_number) {
            // If this isn't the cosign we're currently working on, meaning it's fresh
            if self.current_cosign.as_ref() != Some(&cosign) {
              // Retire the current cosign
              if let Some(current_cosign) = &self.current_cosign {
                assert!(current_cosign.block_number < cosign.block_number);
                self
                  .attempt_manager
                  .retire(&mut txn, VariantSignId::Cosign(current_cosign.block_number));
              }

              // Set the cosign being worked on
              self.current_cosign = Some(cosign.clone());

              let mut machines = Vec::with_capacity(self.keys.len());
              {
                let message = cosign.signature_message();
                for keys in &self.keys {
                  machines.push(WrappedSchnorrkelMachine::new(
                    keys.clone(),
                    COSIGN_CONTEXT,
                    message.clone(),
                  ));
                }
              }
              for msg in
                self.attempt_manager.register(VariantSignId::Cosign(cosign.block_number), machines)
              {
                CosignerToCoordinatorMessages::send(&mut txn, self.session, &msg);
              }

              txn.commit();
            }
          }
        }
      }

      // Handle any messages sent to us
      loop {
        let mut txn = self.db.txn();
        let Some(msg) = CoordinatorToCosignerMessages::try_recv(&mut txn, self.session) else {
          break;
        };
        iterated = true;

        match self.attempt_manager.handle(msg) {
          Response::Messages(msgs) => {
            for msg in msgs {
              CosignerToCoordinatorMessages::send(&mut txn, self.session, &msg);
            }
          }
          Response::Signature { id, signature } => {
            let VariantSignId::Cosign(block_number) = id else {
              panic!("CosignerTask signed a non-Cosign")
            };
            assert_eq!(
              Some(block_number),
              self.current_cosign.as_ref().map(|cosign| cosign.block_number)
            );

            let cosign = self.current_cosign.take().unwrap();
            LatestCosigned::set(&mut txn, self.session, &cosign.block_number);
            let cosign = SignedCosign {
              cosign,
              signature: Signature::from(signature).encode().try_into().unwrap(),
            };
            // Send the cosign
            Cosign::send(&mut txn, self.session, &cosign);
          }
        }

        txn.commit();
      }

      Ok(iterated)
    }
  }
}
