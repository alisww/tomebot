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
use reqwest::StatusCode;


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
                          Ok(resp) => {
                              if resp.status() == StatusCode::TOO_MANY_REQUESTS {
                                  if let Some(time_left) = resp.headers().get("X-RateLimit-Reset-After") {
                                      let secs = time_left.to_str().unwrap().parse::<f64>().unwrap();
                                      thread::sleep(Duration::from_secs_f64(secs));
                                  }
                              }
                          },
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

    listen_db.execute("LISTEN new_events", &[]).unwrap();

    let mut notifications = listen_db.notifications();
    let mut iter = notifications.blocking_iter();

    while let res = iter.next() {
        match res {
            Ok(maybe_notif) => {
                if let Some(n) = maybe_notif {
                    let doc_id = Uuid::parse_str(n.payload()).unwrap();
                    match db.query_one(
                        "SELECT object FROM documents WHERE doc_id = $1::uuid LIMIT 1",
                        &[&doc_id],
                    ) {
                        Ok(event) => {
                            let obj = event.get::<&str, JSONValue>("object");
                            match db.query(format!("SELECT true FROM documents WHERE object @@ '$.type == {}' LIMIT 2",obj["type"].as_i64().unwrap()).as_str(), &[]) {
                                Ok(events) => {
                                    if events.len() < 2 {
                                        tx.send(json!({
                                                "username": username,
                                                "embeds": [
                                                    {
                                                        "title": format!("noticed new event type: {}", obj["type"].as_i64().unwrap()),
                                                        "description": format!("```json\n{}\n```",serde_json::to_string_pretty(&obj).unwrap())
                                                    }
                                                ]
                                        }).to_string());
                                    }
                                },
                                Err(e) => {
                                    error!("couldn't check on events with same type - {:?}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("couldn't find event for doc_id - {:?}", e);
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
