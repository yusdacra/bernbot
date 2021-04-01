fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    if let Some(cmd) = std::env::args().nth(1) {
        match cmd.as_str() {
            #[cfg(feature = "discord")]
            "discord" => {
                println!("running discord impl");
                runtime.block_on(bernbot::discord::discord_main(runtime.handle().clone()))
            }
            x => println!("impl {} not found", x),
        }
    } else {
        print!("please choose an impl\ncurrent are: ");
        #[cfg(feature = "discord")]
        print!("discord");
        println!();
    }

    runtime.shutdown_background();
}
