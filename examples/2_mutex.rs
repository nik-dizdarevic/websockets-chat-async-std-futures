use std::sync::Arc;
use async_std::sync::Mutex;
use async_std::task;

#[async_std::main]
async fn main() {
    // kreiramo in ovijemo Mutex v Arc
    let data1 = Arc::new(Mutex::new(0));

    // kloniramo Arc
    let data2 = Arc::clone(&data1);

    let handle = task::spawn(async move {
        // pridobimo zaklep
        let mut guard = data2.lock().await;
        *guard += 1;
    });

    handle.await;

    println!("Result: {}", *data1.lock().await);
}