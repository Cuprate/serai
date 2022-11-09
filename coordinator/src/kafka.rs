mod crypt;

use std::{thread, time::Duration, collections::HashMap};
use std::env;
use rdkafka::{
  consumer::{BaseConsumer, Consumer, ConsumerContext, Rebalance},
  message::ToBytes,
  producer::{BaseProducer, BaseRecord, Producer, ProducerContext, ThreadedProducer},
  ClientConfig, ClientContext, Message, Offset,
};

use k256::{
  elliptic_curve::{ops::Reduce, sec1::ToEncodedPoint, sec1::Tag},
  ProjectivePoint, U256,
  sha2::{Digest, Sha256},
};

use frost::{
  curve::Secp256k1,
  algorithm::Schnorr,
  tests::{algorithm_machines, key_gen, sign},
};

use rand_core::OsRng;

use message_box::MessageBox;
use dalek_ff_group::{Scalar, RistrettoPoint};
use k256::elliptic_curve::Group;
use dalek_ff_group::dalek::ristretto::RistrettoPoint as OtherRistrettoPoint;

pub struct EncryptedMessage {
  //pub counter_parties: HashMap<String, String>,
  //pub encrpy_to: String,
  //pub decrypt_from: String,
}

impl SeraiCrypt for EncryptedMessage {}

pub fn create_message_box() {
  // our_name: static string
  let our_name = "serai_message";

  // Use message box lib to create private/public keys
  let key_gen = message_box::key_gen();

  // our_key: Scalar
  let our_key = key_gen.0;

  // pub_keys: HashMap<&'static str, RistrettoPoint>,
  let mut pub_keys = HashMap::new();

  // Used to query pub key in message box
  let pub_key_query = "key";

  // Insert query name and pub key (RistrettoPoint)
  pub_keys.insert(pub_key_query, key_gen.1);

  // Using our_name, our_key, & pub key, initialize a message box
  let message_box = MessageBox::new(our_name, our_key, pub_keys);
  //dbg!(&message_box);

  // Create message to be encrypted
  let message = "Message to be encrypted";
  let message_as_vec = message.as_bytes().to_vec();

  // Pass pub key query & message for encryption
  let secure_message = message_box.encrypt(pub_key_query, message_as_vec);
  //dbg!(&secure_message);

  // Pass pub key query and secure message to decrypt
  let res = message_box.decrypt(pub_key_query, secure_message);
  //dbg!(res);
}

pub fn start() {
  // Set an encryption key used for decrypting messages as environment variable
  let key = "ENCRYPT_KEY";
  env::set_var(key, "magickey");

  let consumer: BaseConsumer<ConsumerCallbackLogger> = ClientConfig::new()
    .set("bootstrap.servers", "localhost:9094")
    //for auth
    /*.set("security.protocol", "SASL_SSL")
    .set("sasl.mechanisms", "PLAIN")
    .set("sasl.username", "<update>")
    .set("sasl.password", "<update>")*/
    .set("group.id", "serai")
    .create_with_context(ConsumerCallbackLogger {})
    .expect("invalid consumer config");

  consumer.subscribe(&["test_topic"]).expect("topic subscribe failed");

  thread::spawn(move || loop {
    for msg_result in consumer.iter() {
      let msg = msg_result.unwrap();
      let key: &str = msg.key_view().unwrap().unwrap();
      let value = msg.payload().unwrap();
      // let message_box = MessageBox::new(&static str , dalek_ff_group::Scalar, HashMap<&static str, dalek_ff_group::RistrettoPoint>);
      let encrypted_string = std::str::from_utf8(&value).unwrap();
      let decrypted_string = EncryptedMessage::decrypt(&encrypted_string);
      let user: User =
        serde_json::from_str(&decrypted_string).expect("failed to deserialize JSON to User");
      //println!("{}", decrypted_string);
      println!(
        "received key {} with value {:?} in offset {:?} from partition {}",
        key,
        user,
        msg.offset(),
        msg.partition()
      )
    }
  });

  let producer: ThreadedProducer<ProduceCallbackLogger> = ClientConfig::new()
    .set("bootstrap.servers", "localhost:9094")
    //for auth
    /*.set("security.protocol", "SASL_SSL")
    .set("sasl.mechanisms", "PLAIN")
    .set("sasl.username", "<update>")
    .set("sasl.password", "<update>")*/
    .create_with_context(ProduceCallbackLogger {})
    .expect("invalid producer config");

  // let msg_box = MessageBox {
  //   our_name: val,
  //   our_key: val,
  //   our_public_key: val,
  //   additional_entropy: val,
  //   pub_keys: val,
  //   enc_keys: val
  // };

  for i in 1..100 {
    println!("sending message");

    let user = User { id: i, email: format!("user-{}@foobar.com", i) };

    let user_json = serde_json::to_string_pretty(&user).expect("json serialization failed");

    let encrypted_user = EncryptedMessage::encrypt(&user_json);

    producer
      .send(BaseRecord::to("test_topic").key(&format!("user-{}", i)).payload(&encrypted_user))
      .expect("failed to send message");

    thread::sleep(Duration::from_secs(3));
  }
}

use serde::{Deserialize, Serialize};

use self::crypt::SeraiCrypt;
#[derive(Serialize, Deserialize, Debug)]
struct User {
  id: i32,
  email: String,
}

struct ConsumerCallbackLogger;

impl ClientContext for ConsumerCallbackLogger {}

impl ConsumerContext for ConsumerCallbackLogger {
  fn pre_rebalance<'a>(&self, _rebalance: &rdkafka::consumer::Rebalance<'a>) {}

  fn post_rebalance<'a>(&self, rebalance: &rdkafka::consumer::Rebalance<'a>) {
    println!("post_rebalance callback");

    match rebalance {
      Rebalance::Assign(tpl) => {
        for e in tpl.elements() {
          println!("rebalanced partition {}", e.partition())
        }
      }
      Rebalance::Revoke(tpl) => {
        println!("ALL partitions have been REVOKED")
      }
      Rebalance::Error(err_info) => {
        println!("Post Rebalance error {}", err_info)
      }
    }
  }

  fn commit_callback(
    &self,
    result: rdkafka::error::KafkaResult<()>,
    offsets: &rdkafka::TopicPartitionList,
  ) {
    match result {
      Ok(_) => {
        for e in offsets.elements() {
          match e.offset() {
            //skip Invalid offset
            Offset::Invalid => {}
            _ => {
              println!("committed offset {:?} in partition {}", e.offset(), e.partition())
            }
          }
        }
      }
      Err(err) => {
        println!("error committing offset - {}", err)
      }
    }
  }
}

struct ProduceCallbackLogger;

impl ClientContext for ProduceCallbackLogger {}

impl ProducerContext for ProduceCallbackLogger {
  type DeliveryOpaque = ();

  fn delivery(
    &self,
    delivery_result: &rdkafka::producer::DeliveryResult<'_>,
    _delivery_opaque: Self::DeliveryOpaque,
  ) {
    let dr = delivery_result.as_ref();
    //let msg = dr.unwrap();

    match dr {
      Ok(msg) => {
        let key: &str = msg.key_view().unwrap().unwrap();
        println!(
          "produced message with key {} in offset {} of partition {}",
          key,
          msg.offset(),
          msg.partition()
        )
      }
      Err(producer_err) => {
        let key: &str = producer_err.1.key_view().unwrap().unwrap();

        println!("failed to produce message with key {} - {}", key, producer_err.0,)
      }
    }
  }
}
