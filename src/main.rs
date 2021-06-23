use chrono::prelude::*;
use log::{error, debug};
use postgres::fallible_iterator::FallibleIterator;
use postgres::{Client as DBClient, NoTls};
use serde_json::{json, Value as JSONValue};
use std::env;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use std::sync::mpsc;

fn main() {
    env_logger::init();

    let (tx, rx): (mpsc::Sender<String>, mpsc::Receiver<String>) = mpsc::channel();
    let webhook_urls: Vec<String> = env::var("WEBHOOK_URL").unwrap().split(',').map(|s| s.to_string()).collect();
    let webhook_delay =  Duration::from_millis((&env::var("WEBHOOK_DELAY").unwrap()).parse::<u64>().unwrap());

    let webhook_thread = thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        let last_send = Instant::now();
        for message in rx {
            if last_send.elapsed() < webhook_delay {
                thread::sleep(webhook_delay - last_send.elapsed());
            }

            debug!("Sending webhook {}",message);

            for url in &webhook_urls {
                match client.post(url)
                      .header("Content-Type","application/json")
                      .body(message.clone())
                      .send() {
                          Ok(_) => {},
                          Err(e) => {
                              error!("couldn't wobhook: {:?}",e);
                          }
                };
            }
        };
    });

    let username = env::var("WEBHOOK_USER").unwrap_or("tomebot".to_owned());

    let mut db = DBClient::connect(&env::var("DB_URL").unwrap(), NoTls).unwrap();
    let mut listen_db = DBClient::connect(&env::var("DB_URL").unwrap(), NoTls).unwrap();

    listen_db.execute("LISTEN changed_events", &[]).unwrap();

    let mut notifications = listen_db.notifications();
    let mut iter = notifications.blocking_iter();

    while let res = iter.next() {
        match res {
            Ok(maybe_notif) => {
                if let Some(n) = maybe_notif {
                    let ev_hash = n.payload();
                    match db.query(
                        "SELECT * FROM versions WHERE hash = $1 ORDER BY observed DESC LIMIT 2",
                        &[&ev_hash],
                    ) {
                        Ok(events) => {
                            if events.len() > 1 {
                                let new = &events[0];
                                let old = &events[1];
                                let new_json = new.get::<&str, JSONValue>("object");
                                let old_json = old.get::<&str, JSONValue>("object");

                                let new_json_pretty = serde_json::to_string_pretty(&new_json).unwrap();
                                let old_json_pretty = serde_json::to_string_pretty(&old_json).unwrap();

                                let diff = diffy::create_patch(
                                    &old_json_pretty,
                                    &new_json_pretty
                                );


                                tx.send(json!({
                                    "username": username,
                                    "embeds": [
                                            {
                                                "title": format!("noticed change in event {} at {}",
                                                            new.get::<&str,Uuid>("doc_id").to_string(),
                                                            Utc.timestamp(new.get::<&str,i64>("observed") / 1000,0).to_rfc2822()
                                                        ),
                                                "description": format!("```diff\n{}\n```",diff)
                                            }
                                        ]
                                    }).to_string()
                                );
                            } else {
                                let new = &events[0];
                                let new_json = new.get::<&str, JSONValue>("object");

                                if new_json["season"].as_i64().unwrap_or(20) < 15 {
                                    let new_json_pretty = serde_json::to_string_pretty(&new_json).unwrap();

                                    tx.send(json!({
                                        "username": username,
                                        "embeds": [
                                                {
                                                    "title": format!("noticed new event {} at {}",
                                                                new.get::<&str,Uuid>("doc_id").to_string(),
                                                                Utc.timestamp(new.get::<&str,i64>("observed") / 1000,0).to_rfc2822()
                                                            ),
                                                    "description": format!("```json\n{}\n```",new_json_pretty)
                                                }
                                            ]
                                        }).to_string()
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            error!("couldn't find event for notified hash - {:?}", e);
                        }
                    }
                }
            }
            Err(e) => {
                error!("postgres why >: {:?}", e);
                panic!("{:?}",e);
            }
        }
    }
}
