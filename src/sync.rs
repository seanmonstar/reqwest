use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::time::Duration;

use hyper::{self, Control, Next, Method, StatusCode, HttpVersion, RequestUri, Url};
use hyper::header::Headers;

pub struct Client {
    inner: hyper::Client<SynchronousHandler>,
}

impl Client {
    pub fn new() -> Client {
        Client {
            inner: hyper::Client::<SynchronousHandler>::configure()
                .connect_timeout(Duration::from_secs(10))
                .build().unwrap(),
        }
    }

    pub fn request(&self, method: Method, url: Url, version: HttpVersion, headers: Headers) -> Result<Request, String> {
        let (ctrl_tx, ctrl_rx) = mpsc::channel();
        let (res_tx, res_rx) = mpsc::channel();
        let (action_tx, rx) = mpsc::channel();
        let (tx, action_rx) = mpsc::channel();

        let timeout = Duration::from_secs(10);

        try!(self.inner.request(url, SynchronousHandler {
            read_timeout: timeout,
            write_timeout: timeout,

            ctrl_tx: ctrl_tx,
            res_tx: res_tx,
            tx: tx,
            rx: rx,
            reading: None,
            writing: None,
            request: Some((method, version, headers)),
        }).map_err(|e| format!("RequestError: {}", e)));

        Ok(Request {
            res_rx: res_rx,
            tx: action_tx,
            rx: action_rx,
            ctrl: try!(ctrl_rx.recv().map_err(|e| format!("RequestError: waiting for Control: {}", e))),
        })
    }
}

pub struct Request {
    res_rx: mpsc::Receiver<hyper::client::Response>,
    tx: mpsc::Sender<Action>,
    rx: mpsc::Receiver<io::Result<usize>>,
    ctrl: hyper::Control,
}

impl Request {
    pub fn end(self) -> Result<Response, String> {
        trace!("Request.end");
        self.ctrl.ready(Next::read()).unwrap();
        let res = try!(self.res_rx.recv().map_err(|e| format!("RequestError: end = {}", e)));
        Ok(Response {
            status: res.status().clone(),
            headers: res.headers().clone(),
            version: res.version().clone(),

            tx: self.tx,
            rx: self.rx,
            ctrl: self.ctrl,
        })
    }
}

impl Write for Request {
    fn write(&mut self, msg: &[u8]) -> io::Result<usize> {
        self.tx.send(Action::Write(msg.as_ptr(), msg.len())).unwrap();
        self.ctrl.ready(Next::write()).unwrap();
        let res = self.rx.recv().unwrap();
        res
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("Request.flush() not implemented")
    }
}

pub struct Response {
    pub headers: Headers,
    pub status: StatusCode,
    pub version: HttpVersion,

    tx: mpsc::Sender<Action>,
    rx: mpsc::Receiver<io::Result<usize>>,
    ctrl: hyper::Control,

}

impl Read for Response {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.tx.send(Action::Read(buf.as_mut_ptr(), buf.len())).unwrap();
        self.ctrl.ready(Next::read()).unwrap();
        self.rx.recv().unwrap()
    }
}

struct SynchronousHandler {
    read_timeout: Duration,
    write_timeout: Duration,

    ctrl_tx: mpsc::Sender<Control>,
    res_tx: mpsc::Sender<hyper::client::Response>,
    tx: mpsc::Sender<io::Result<usize>>,
    rx: mpsc::Receiver<Action>,
    reading: Option<(*mut u8, usize)>,
    writing: Option<(*const u8, usize)>,
    request: Option<(hyper::Method, hyper::HttpVersion, hyper::Headers)>
}

unsafe impl Send for SynchronousHandler {}

impl SynchronousHandler {
    fn next(&mut self) -> Next {
        match self.rx.try_recv() {
            Ok(Action::Read(ptr, len)) => {
                self.reading = Some((ptr, len));
                Next::read().timeout(self.read_timeout)
            },
            Ok(Action::Write(ptr, len)) => {
                self.writing = Some((ptr, len));
                Next::write().timeout(self.write_timeout)
            }
            Err(mpsc::TryRecvError::Empty) => {
                // we're too fast, the other thread hasn't had a chance to respond
                Next::wait()
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                // they dropped it
                Next::end()
            }
        }
    }

    fn reading(&mut self) -> Option<(*mut u8, usize)> {
        self.reading.take().or_else(|| {
            match self.rx.try_recv() {
                Ok(Action::Read(ptr, len)) => {
                    Some((ptr, len))
                },
                _ => None
            }
        })
    }

    fn writing(&mut self) -> Option<(*const u8, usize)> {
        self.writing.take().or_else(|| {
            match self.rx.try_recv() {
                Ok(Action::Write(ptr, len)) => {
                    Some((ptr, len))
                },
                _ => None
            }
        })
    }
}

impl hyper::client::Handler<hyper::client::DefaultTransport> for SynchronousHandler {
    fn on_request(&mut self, req: &mut hyper::client::Request) -> Next {
        use std::iter::Extend;
        let head = self.request.take().unwrap();
        trace!("on_request {:?}", head);
        req.set_method(head.0);
        //req.set_uri(head.1);
        req.headers_mut().extend(head.2.iter());
        self.next()

    }

    fn on_request_writable(&mut self, encoder: &mut hyper::Encoder<hyper::client::DefaultTransport>) -> Next {
        trace!("on_request_writable");
        if let Some(raw) = self.writing() {
            let slice = unsafe { ::std::slice::from_raw_parts(raw.0, raw.1) };
            if self.tx.send(encoder.write(slice)).is_err() {
                return Next::end();
            }
        }
        self.next()
    }

    fn on_response(&mut self, res: hyper::client::Response) -> Next {
        trace!("on_response {:?}", res);
        if let Err(_) = self.res_tx.send(res) {
            return Next::end();
        }
        self.next()
    }

    fn on_response_readable(&mut self, decoder: &mut hyper::Decoder<hyper::client::DefaultTransport>) -> Next {
        trace!("on_response_readable");
        if let Some(raw) = self.reading() {
            let slice = unsafe { ::std::slice::from_raw_parts_mut(raw.0, raw.1) };
            if self.tx.send(decoder.read(slice)).is_err() {
                return Next::end();
            }
        }
        self.next()
    }

    fn on_control(&mut self, ctrl: Control) {
        self.ctrl_tx.send(ctrl).unwrap();
    }
}

enum Action {
    Read(*mut u8, usize),
    Write(*const u8, usize),
    //Request(Method, RequestUri, HttpVersion, Headers),
}

unsafe impl Send for Action {}


#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn test_get() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        thread::spawn(move || {
            let mut inc = server.accept().unwrap().0;
            let mut buf = [0; 4096];
            inc.read(&mut buf).unwrap();
        });

        let mut res = super::super::get(&format!("http://{}", addr)).unwrap();
        assert_eq!(res.status(), &::hyper::Ok);

        let mut buf = Vec::new();
        res.read_to_end(&mut buf).unwrap();
    }
}
