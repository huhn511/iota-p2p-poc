use serde::{Serialize, Deserialize};
use identity_core::did::{DID};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum DIDComm {
    TrustPing,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct TrustPing {
    pub did: DID,
}

// #[derive(Serialize, Deserialize, Debug, PartialEq)]
// pub struct TrustPing {
//     pub did: DID,
//     //type here?
//     pub type: String,
//     pub response_requested: bool,
//     pub signature: Option<String>,
// }


// Message Wrapper?
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Message {
    pub did: String,
    #[serde(rename = "type")]
    pub _type: String,
    // pub payload: DIDComm::Trustping(Trustping),
}


// DIDComm Messaging
// https://github.com/decentralized-identity/didcomm-messaging/blob/master/jwm.md