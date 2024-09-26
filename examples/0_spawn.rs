use async_std::task;

#[async_std::main]
async fn main() {
    let handle = task::spawn(async {
        // tukaj opravimo nekaj asinhronega dela
        "return value"
    });

    // tukaj opravimo nekaj drugega dela

    let out = handle.await;
    println!("Got {}", out);
}