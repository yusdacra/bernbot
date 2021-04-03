fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    #[cfg(feature = "discord")]
    runtime.block_on(bernbot::discord::main(runtime.handle().clone()));

    runtime.shutdown_background();
}
