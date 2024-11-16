use std::time::Duration;
use async_std::channel;
use async_std::task;

#[async_std::main]
async fn main() {
    // kreiramo kanal
    let (tx, rx) = channel::unbounded();

    // kloniramo oddajnik
    let tx2 = tx.clone();

    task::spawn(async move {
        // počakamo eno sekundo
        task::sleep(Duration::from_secs(1)).await;
        // pošljemo vrednost
        tx.send("sending from first handle").await.unwrap();
    });

    task::spawn(async move {
        // pošljemo vrednost
        tx2.send("sending from second handle").await.unwrap();
    });

    // sprejmemo vrednost
    while let Ok(message) = rx.recv().await {
        println!("Got = {}", message);
    }
}