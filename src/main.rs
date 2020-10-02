use crate::command_protocol::{CommandCodec, CommandProtocol, CommandRequest};
use crate::network_behaviour::P2PNetworkBehaviour;
use async_std::{
    io::{stdin, BufReader},
    task,
};
use futures::{future, prelude::*};
use libp2p::{
    build_development_transport,
    core::Multiaddr,
    identity::Keypair,
    kad::{record::store::MemoryStore, Kademlia},

    mdns::Mdns,
    request_response::{ProtocolSupport, RequestResponse, RequestResponseConfig},
    swarm::{ExpandedSwarm, IntoProtocolsHandler, NetworkBehaviour, ProtocolsHandler},
    PeerId, Swarm,
};
use std::{
    error::Error,
    iter,
    str::{FromStr, SplitWhitespace},
    string::String,
    task::{Context, Poll},
};

use identity_core::did::{DID};
use identity_common::Timestamp;
use identity_comm::did_comm::{TrustPing};
use identity_comm::types::{TRUSTPING};
use identity_comm::DIDComm_message;

mod dht_proto {
    include!(concat!(env!("OUT_DIR"), "/dht.pb.rs"));
}
mod command_protocol;
mod network_behaviour;

type P2PNetworkSwarm = ExpandedSwarm<
    P2PNetworkBehaviour,
    <<<P2PNetworkBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent,
    <<<P2PNetworkBehaviour as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent,
    <P2PNetworkBehaviour as NetworkBehaviour>::ProtocolsHandler,
    PeerId,
>;

fn main() -> Result<(), Box<dyn Error>> {
    // Create a random PeerId
    let local_keys = Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_keys.public());
    println!("Local peer id: {:?}", local_peer_id);

    let did = DID {
        method_name: "iota".into(),
        id_segments: vec![local_peer_id.to_base58()],
        ..Default::default()
    }
    .init()
    .unwrap();
    println!("did: {}", did.to_string());

    // create a transport
    let transport = build_development_transport(local_keys)?;

    // Create a Kademlia behaviour.
    let kademlia = {
        let store = MemoryStore::new(local_peer_id.clone());
        Kademlia::new(local_peer_id.clone(), store)
    };
    let mdns = Mdns::new()?;

    // Create RequestResponse behaviour with CommandProtocol
    let msg_proto = {
        // set request_timeout and connection_keep_alive if necessary
        let cfg = RequestResponseConfig::default();
        let protocols = iter::once((CommandProtocol(), ProtocolSupport::Full));
        RequestResponse::new(CommandCodec(), protocols, cfg)
    };
    // Create a Swarm that establishes connections through the given transport
    // Use custom behaviour P2PNetworkBehaviour
    let mut swarm = {
        let behaviour = P2PNetworkBehaviour {
            kademlia,
            mdns,
            msg_proto,
        };
        Swarm::new(transport, behaviour, local_peer_id)
    };

    let mut is_swarm_listening = false;
    if let Some(i) = std::env::args().position(|arg| arg == "--port") {
        if let Some(port) = std::env::args().nth(i + 1) {
            let addr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
            Swarm::listen_on(&mut swarm, addr)?;
            is_swarm_listening = true;
        }
    }

    if !is_swarm_listening {
        #[cfg(not(feature = "server"))]
        // Tell the swarm to listen on all interfaces and a random, OS-assigned port.
        Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse()?)?;

        #[cfg(feature = "server")]
        Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/16384".parse()?)?;
    }

    attempt_dialing_remote(&mut swarm);

    poll_input(swarm)
}

fn attempt_dialing_remote(swarm: &mut P2PNetworkSwarm) {
    if let Some(i) = std::env::args().position(|arg| arg == "--dial") {
        // Dial peer at fixed addr to connect to p2p network
        if let Some(addr) = std::env::args().nth(i + 1) {
            if let Ok(remote) = Multiaddr::from_str(&*addr) {
                if Swarm::dial_addr(swarm, remote.clone()).is_ok() {
                    println!("Dialed {}", addr);
                    if let Some(peer_id) = std::env::args().nth(i + 2) {
                        if let Ok(peer) = PeerId::from_str(&*peer_id) {
                            swarm.kademlia.add_address(&peer, remote);
                            if swarm.kademlia.bootstrap().is_ok() {
                                println!("Successful bootstrapping");
                            } else {
                                eprintln!("Could not bootstrap");
                            }
                        } else {
                            eprintln!("Invalid Peer Id {}", peer_id);
                        }
                    }
                } else {
                    eprintln!("Could not dial {}", addr);
                }
            } else {
                eprintln!("Invalid multiaddress {}", addr);
            }
        } else {
            eprintln!("Missing multiaddress");
        }
    }
}

fn poll_input(mut swarm: P2PNetworkSwarm) -> Result<(), Box<dyn Error>> {
    let mut stdin = BufReader::new(stdin()).lines();
    let mut listening = false;
    task::block_on(future::poll_fn(move |cx: &mut Context<'_>| {
        loop {
            // poll for user input in stdin
            match stdin.try_poll_next_unpin(cx)? {
                Poll::Ready(Some(line)) => handle_input_line(&mut swarm, line),
                Poll::Ready(None) => panic!("Stdin closed"),
                Poll::Pending => break,
            }
        }
        loop {
            match swarm.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => println!("{:?}", event),
                Poll::Ready(None) => {
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => {
                    if !listening {
                        for a in Swarm::listeners(&swarm) {
                            println!("Listening on {:?}", a);
                        }
                        listening = true;
                        println!("Type LIST to view current bucket entries");
                        println!("Type PING <peer_id> to ping another peer");
                        println!("Type CMD <peer_id> <message> to send a command / message to another peer");
                    }
                    break;
                }
            }
        }
        Poll::Pending
    }))
}

fn handle_input_line(swarm: &mut P2PNetworkSwarm, line: String) {
    let mut args = line.split_whitespace();
    match args.next() {
        Some("PING") => send_ping_to_peer(args, &mut swarm.msg_proto),
        Some("TRUSTPING") => send_trustping_to_peer(args, &mut swarm.msg_proto),
        Some("CMD") => send_cmd_to_peer(args, &mut swarm.msg_proto),
        Some("LIST") => {
            println!("Current Buckets:");
            for bucket in swarm.kademlia.kbuckets() {
                for entry in bucket.iter() {
                    println!(
                        "key: {:?}, values: {:?}",
                        entry.node.key.preimage(),
                        entry.node.value
                    );
                }
            }
        }
        _ => println!("No valid command"),
    }
}

fn send_ping_to_peer(mut args: SplitWhitespace, msg_proto: &mut RequestResponse<CommandCodec>) {
    if let Some(peer_id) = args.next() {
        if let Ok(peer) = PeerId::from_str(peer_id) {
            let ping = CommandRequest::Ping;
            println!("Sending Ping to peer {:?}", peer);
            msg_proto.send_request(&peer, ping);
        } else {
            println!("Faulty target peer id");
        }
    } else {
        println!("Expected target peer id");
    }
}

fn send_trustping_to_peer(mut args: SplitWhitespace, msg_proto: &mut RequestResponse<CommandCodec>) {
    if let Some(peer_id) = args.next() {
        if let Ok(peer) = PeerId::from_str(peer_id) {
            let ping = CommandRequest::Ping;
            println!("Sending Ping to peer {:?}", peer);
            msg_proto.send_request(&peer, ping);
        } else {
            println!("Faulty target peer id");
        }
    } else {
        println!("Expected target peer id");
    }
}

fn send_cmd_to_peer(mut args: SplitWhitespace, msg_proto: &mut RequestResponse<CommandCodec>) {
    if let Some(peer_id) = args.next() {
        if let Ok(peer) = PeerId::from_str(peer_id) {
            let cmd = {
                match args.next() {
                    Some(c) => c,
                    None => {
                        println!("Expected command");
                        ""
                    }
                }
            };
            let other = CommandRequest::Other(cmd.as_bytes().to_vec());
            println!("Sending command {:?} to peer: {:?}", cmd, peer);

            match cmd {
                "TRUSTPING" => {
                    // IF CMD == Trustping
                    println!("send TRUSTPING command");
                    let did = DID {
                        method_name: "iota".into(),
                        id_segments: vec![peer_id.into()],
                        ..Default::default()
                    }
                    .init()
                    .unwrap();
                    // println!("did: {}", did.to_string());
                    // let trustping_string = serde_json::to_string(&TrustPing{response_requested: true}).unwrap();

                    // change "TRUSTPING" with DIDComm Message type
                    //let trustping = CommandRequest::Other("TRUSTPING DID".to_string().as_bytes().to_vec());
                    

                    let mut did_comm_message: DIDComm_message = DIDComm_message::new();
                    did_comm_message.set_id(Timestamp::now().to_rfc3339().to_string());
                    
                    
                    did_comm_message.set_from(did.to_string());
                    


                    // did_comm_message.set_type(types.TrustPing); // https:://didcomm.org/v1/messages/TrustPing
                    
                    
                    did_comm_message.set_type(TRUSTPING); // https:://didcomm.org/v1/messages/TrustPing

                    
                    let ping = TrustPing { response_requested: true };                    
                    let value = serde_json::to_value(ping).unwrap();
                    let object = value.as_object().unwrap();
                    did_comm_message.set_body(object.clone());

                    println!("did_comm_message: {:?}", did_comm_message);
                    
                    // TODO: sign message
                    

                    let did_comm_message_string = serde_json::to_string(&did_comm_message).unwrap();

                    // add trustping to did_comm_message body
                    let did_comm_message_request = CommandRequest::Other(did_comm_message_string.as_bytes().to_vec());


                    // send request for did_comm_message 
                    msg_proto.send_request(&peer, did_comm_message_request);
                    
                },
                _ => {
                    println!("send default command");
                    msg_proto.send_request(&peer, other);
                }
            }


        } else {
            println!("Faulty target peer id");
        }
    } else {
        println!("Expected target peer id");
    }
}
