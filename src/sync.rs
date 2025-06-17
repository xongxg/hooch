pub mod mpsc;

#[cfg(test)]
mod tests {
    use crate::runtime::RuntimeBuilder;
    use crate::sync::mpsc::bounded_channel;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_bounded_channels() {
        let (mytx, myrx) = bounded_channel::<i32>(1);
        let runtime_handle = RuntimeBuilder::default().build();

        std::thread::Builder::new()
            .name("sender".into())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let _ = mytx.send(42);
            });

        let result = Arc::new(Mutex::new(0));
        let result_clone = Arc::clone(&result);

        runtime_handle.run_blocking(async move {
            let rx_res = myrx.recv().await;
            *result_clone.lock().unwrap() = rx_res.unwrap();
        });
        
        println!("result: {:?}", result.lock().unwrap());
    }
}
