use crate::pool::thread_pool::HoochPool;
use crate::reactor::Reactor;
use mio::{Interest, Token};
use std::io;
use std::io::{ErrorKind, Read, Write};
use std::net::ToSocketAddrs;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// An asynchronous TCP stream that wraps a non-blocking `mio::net::TcpStream`.
///
/// [`HoochTcpStream`] leverages a custom reactor to handle I/O readiness events and a thread pool
/// to offload the initial blocking connection operation. It provides asynchronous methods to
/// connect, read from, and write to the underlying stream.
pub struct HoochTcpStream {
    pub stream: mio::net::TcpStream,
    pub token: Token,
}

impl HoochTcpStream {
    /// Asynchronously connects to the specified socket address and returns a new [`HoochTcpStream`].
    ///
    /// # Arguments
    ///
    /// * `addr` - An address that can be converted into a socket address (e.g., a `&str` or `SocketAddr`).
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect(addr: impl ToSocketAddrs) -> std::io::Result<Self> {
        // Generate a unique reactor tag for this connection attempt.
        let reactor_tag = Reactor::generate_reactor_tag();
        let reactor = Reactor::get();
        reactor.register_reactor_tag(reactor_tag);

        // Shared state to store the connection result from the thread pool.
        let stream_handle: Arc<Mutex<Option<Result<mio::net::TcpStream, io::Error>>>> =
            Arc::new(Mutex::new(None));

        let mut async_hooch_tcp_stream = Box::pin(AsyncHoochTcpStream {
            addr,
            state: Arc::clone(&stream_handle),
            has_polled: false,
        });

        // Poll the connection future until it is ready.
        let mut stream =
            std::future::poll_fn(|cx| async_hooch_tcp_stream.as_mut().poll(cx)).await?;

        // Get a unique token from the reactor to register the stream for I/O events.
        let reactor = Reactor::get();
        let token = reactor.unique_token();

        Reactor::get().registry().register(
            &mut stream,
            token,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        Ok(Self { stream, token })
    }

    /// Asynchronously reads data from the TCP stream into the provided buffer.
    ///
    /// This method will poll the stream until it becomes readable. It then reads data into the buffer
    /// and returns the number of bytes read.
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable byte slice to store the read data.
    ///
    /// # Errors
    ///
    /// Returns an error if the read operation fails.
    pub async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            match self.stream.read(buf) {
                Ok(num) => return Ok(num),
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    std::future::poll_fn(|cx| Reactor::get().poll(self.token, cx)).await?
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Asynchronously writes data from the provided buffer to the TCP stream.
    ///
    /// This method will poll the stream until it becomes writable. It then writes data from the buffer
    /// to the stream and returns the number of bytes written.
    ///
    /// # Arguments
    ///
    /// * `buf` - A byte slice containing the data to write.
    ///
    /// # Errors
    ///
    /// Returns an error if the write operation fails.
    pub async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        loop {
            match self.stream.write(buf) {
                Ok(num) => return Ok(num),
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    std::future::poll_fn(|cx| Reactor::get().poll(self.token, cx)).await?
                }
                Err(err) => return Err(err),
            }
        }
    }
}

struct AsyncHoochTcpStream<T: ToSocketAddrs> {
    addr: T,
    state: Arc<Mutex<Option<Result<mio::net::TcpStream, io::Error>>>>,
    has_polled: bool,
}

impl<T: ToSocketAddrs> Future for AsyncHoochTcpStream<T> {
    type Output = Result<mio::net::TcpStream, io::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.has_polled {
            let this = unsafe { self.as_mut().get_unchecked_mut() };
            this.has_polled = true;

            let stream_handle_clone = Arc::clone(&self.state);
            let socket_addr = self.addr.to_socket_addrs().unwrap().next().unwrap();

            let waker = cx.waker().clone();
            let connect_fn = move || {
                let result = move || {
                    let stream = std::net::TcpStream::connect(socket_addr)?;
                    stream.set_nonblocking(true)?;
                    Ok(mio::net::TcpStream::from_std(stream))
                };

                let stream_result = result();
                *stream_handle_clone.lock().unwrap() = Some(stream_result);
                waker.wake();
            };

            // Execute the connection function in the thread pool.
            let pool = HoochPool::get();
            pool.execute(Box::new(connect_fn));
            return Poll::Pending;
        }

        if self.state.lock().unwrap().is_none() {
            return Poll::Pending;
        }

        let stream_result = self.state.lock().unwrap().take().unwrap();
        Poll::Ready(stream_result)
    }
}
