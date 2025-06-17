use crate::net::tcp::stream::HoochTcpStream;
use crate::pool::thread_pool::HoochPool;
use crate::reactor::Reactor;
use mio::{Interest, Token};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::ops::Deref;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

pub struct HoochTcpListener {
    listener: mio::net::TcpListener,
    token: Token,
}

impl HoochTcpListener {
    /// Asynchronously binds to the provided socket address.
    ///
    /// This method offloads the blocking bind operation to a thread pool, then registers
    /// the resulting listener with the reactor for non-blocking I/O events.
    ///
    /// # Arguments
    ///
    /// * `addr` - An address that can be converted into a socket address (e.g., a `&str` or `SocketAddr`).
    ///
    /// # Errors
    ///
    pub async fn bind(addr: impl ToSocketAddrs) -> std::io::Result<HoochTcpListener> {
        // Generate a reactor tag for this bind operation.
        let reactor_tag = Reactor::generate_reactor_tag();
        let reactor = Reactor::get();
        reactor.register_reactor_tag(reactor_tag);

        let listener_handle: Arc<Mutex<Option<Result<mio::net::TcpListener, io::Error>>>> =
            Arc::new(Mutex::default());

        // Create a future that will attempt to bind the listener asynchronously.
        let mut async_hooch_tcp_listener = Box::pin(AsyncHoochTcpListener {
            addr,
            state: Arc::clone(&listener_handle),
            has_polled: false,
        });

        // Poll the future until the binding operation completes.
        let mut listener =
            std::future::poll_fn(move |cx| async_hooch_tcp_listener.as_mut().poll(cx)).await?;

        let reactor = Reactor::get();
        let token = reactor.unique_token();

        Reactor::get().registry().register(
            &mut listener,
            token,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        Ok(HoochTcpListener { listener, token })
    }

    /// Asynchronously accepts a new incoming TCP connection.
    ///
    /// This method continuously polls the listener until a new connection is accepted.
    /// Upon acceptance, it registers the new connection with the reactor and wraps it in a
    /// [`HoochTcpStream`].
    ///
    /// # Errors
    ///
    /// Returns an error if accepting a connection fails.
    pub async fn accept(&self) -> std::io::Result<(HoochTcpStream, SocketAddr)> {
        loop {
            match self.listener.accept() {
                Ok((mut stream, addr)) => {
                    let reactor = Reactor::get();
                    let token = reactor.unique_token();

                    // Register the accepted stream with the reactor for I/O events.
                    Reactor::get().registry().reregister(
                        &mut stream,
                        token,
                        Interest::READABLE | Interest::WRITABLE,
                    )?;

                    return Ok((HoochTcpStream { stream, token }, addr));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::future::poll_fn(|cx| Reactor::get().poll(self.token, cx)).await?;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Deref for HoochTcpListener {
    type Target = mio::net::TcpListener;

    /// Dereferences to the underlying `mio::net::TcpListener`.
    fn deref(&self) -> &Self::Target {
        &self.listener
    }
}

/// A future that resolves to a non-blocking TCP listener once the bind operation completes.
///
/// This future offloads the blocking `TcpListener::bind` call to a thread pool,
/// then converts the bound listener into a non-blocking `mio::net::TcpListener`.
struct AsyncHoochTcpListener<T: ToSocketAddrs> {
    addr: T,
    state: Arc<Mutex<Option<Result<mio::net::TcpListener, io::Error>>>>,
    has_polled: bool,
}

impl<T: ToSocketAddrs> Future for AsyncHoochTcpListener<T> {
    type Output = Result<mio::net::TcpListener, io::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Initiate the binding operation on the first poll.
        if !self.has_polled {
            let this = unsafe { self.as_mut().get_unchecked_mut() };
            this.has_polled = true;

            let listener_handle_clone = Arc::clone(&self.state);
            let socket_addr = self.addr.to_socket_addrs().unwrap().next().unwrap();

            // Clone the waker to notify the current task once the binding completes.
            let waker = cx.waker().clone();
            let connect_fn = move || {
                let result = move || {
                    let std_listener = std::net::TcpListener::bind(socket_addr)?;
                    std_listener.set_nonblocking(true)?;
                    Ok(mio::net::TcpListener::from_std(std_listener))
                };

                let listener_result = result();
                *listener_handle_clone.lock().unwrap() = Some(listener_result);
                // Wake up the task waiting on this future.
                waker.wake();
            };

            // Execute the binding operation in the thread pool.
            let pool = HoochPool::get();
            pool.execute(Box::new(connect_fn));
            return Poll::Pending;
        }

        // Continue waiting if the binding result is not yet available.
        if self.state.lock().unwrap().is_none() {
            return Poll::Pending;
        }

        // Retrieve and return the binding result.
        let listener_result = self.state.lock().unwrap().take().unwrap();
        Poll::Ready(listener_result)
    }
}
