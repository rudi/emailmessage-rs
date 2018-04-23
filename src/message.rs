use std::fmt::{Display, Formatter, Result as FmtResult};
use std::time::{SystemTime};
use std::io::{Error as IoError, ErrorKind};
use futures::{Sink, Stream, Future};
use hyper::{Body as HyperBody, Chunk as HyperChunk, Error as HyperError};
use tokio_proto::streaming::{Body as StreamingBody};
use header::{Headers, Header, Date, EmailDate};

pub type MailBody = HyperBody;
//pub type MailBody = StreamingBody<Vec<u8>, IoError>;

#[derive(Clone, Debug)]
pub struct Message<B = MailBody> {
    headers: Headers,
    body: Option<B>,
}

impl<B> Message<B> {
    /// Constructs a default message
    #[inline]
    pub fn new() -> Self {
        Message::default().with_date(None)
    }

    /// Get the headers from the Message.
    #[inline]
    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// Get a mutable reference to the headers.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut Headers {
        &mut self.headers
    }

    /// Set a header and move the Message.
    ///
    /// Useful for the "builder-style" pattern.
    #[inline]
    pub fn with_header<H: Header>(mut self, header: H) -> Self {
        self.headers.set(header);
        self
    }

    /// Set the headers and move the Message.
    ///
    /// Useful for the "builder-style" pattern.
    #[inline]
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Set a date and move the Message.
    ///
    /// Useful for the "builder-style" pattern.
    ///
    /// `None` value means use current local time as a date.
    #[inline]
    pub fn with_date(self, date: Option<EmailDate>) -> Self {
        let date: EmailDate = date.unwrap_or_else(|| SystemTime::now().into());
        
        self.with_header(Date(date))
    }

    /// Set the body.
    #[inline]
    pub fn set_body<T: Into<B>>(&mut self, body: T) {
        self.body = Some(body.into());
    }

    /// Set the body and move the Message.
    ///
    /// Useful for the "builder-style" pattern.
    #[inline]
    pub fn with_body<T: Into<B>>(mut self, body: T) -> Self {
        self.set_body(body);
        self
    }

    /// Read the body.
    #[inline]
    pub fn body_ref(&self) -> Option<&B> { self.body.as_ref() }
    
    pub fn streaming<C>(self) -> (StreamingBody<Vec<u8>, IoError>, Box<Future<Item = (), Error = IoError>>)
    where B: Stream<Item = C, Error = HyperError> + 'static,
          C: Into<HyperChunk>,
    {
        let (sender, body) = StreamingBody::pair();

        let sent = sender.send(Ok(Vec::from(self.headers.to_string())))
            .map_err(|_| IoError::new(ErrorKind::BrokenPipe, "Unable to send email headers"));

        (body,
         if let Some(body) = self.body {
             Box::new(sent.and_then(|sender| sender.send_all(body.map(|chunk| Ok(chunk.into().as_ref().into()))
                                                             .map_err(|_| panic!()))
                                    .map_err(|_| IoError::new(ErrorKind::BrokenPipe, "Unable to send email body")))
                      .map(|_| ()))
        } else {
             Box::new(sent
                      .map(|_| ()))
        })
    }
}

impl<B> Default for Message<B> {
    fn default() -> Self {
        Message {
            headers: Headers::default(),
            body: Option::default()
        }
    }
}

impl<B> Display for Message<B>
where B: Display
{
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        self.headers.fmt(f)?;
        if let Some(ref body) = self.body {
            write!(f, "{}", body)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use header;
    use mailbox::{Mailbox};
    use message::{Message};

    use std::str::from_utf8;
    use futures::{Stream, Future};
    use tokio_core::reactor::{Core};
    
    #[test]
    fn date_header() {
        let date = "Tue, 15 Nov 1994 08:12:31 GMT".parse().unwrap();
        
        let email: Message<String> = Message::new()
            .with_date(Some(date))
            .with_body("\r\n");
        
        assert_eq!(format!("{}", email), "Date: Tue, 15 Nov 1994 08:12:31 GMT\r\n\r\n");
    }

    #[test]
    fn email_message() {
        let date = "Tue, 15 Nov 1994 08:12:31 GMT".parse().unwrap();
        
        let email: Message<String> = Message::new()
            .with_date(Some(date))
            .with_header(header::From(vec![Mailbox::new(Some("Каи".into()), "kayo@example.com".parse().unwrap())]))
            .with_header(header::To(vec!["Pony O.P. <pony@domain.tld>".parse().unwrap()]))
            .with_header(header::Subject("яңа ел белән!".into()))
            .with_body("\r\nHappy new year!");
        
        assert_eq!(format!("{}", email),
                   concat!("Date: Tue, 15 Nov 1994 08:12:31 GMT\r\n",
                           "From: =?utf-8?b?0JrQsNC4?= <kayo@example.com>\r\n",
                           "To: Pony O.P. <pony@domain.tld>\r\n",
                           "Subject: =?utf-8?b?0Y/So9CwINC10Lsg0LHQtdC705nQvSE=?=\r\n",
                           "\r\n",
                           "Happy new year!"));
    }

    #[test]
    fn message_streaming() {
        let mut core = Core::new().unwrap();
        let handle = core.handle();
        
        let date = "Tue, 15 Nov 1994 08:12:31 GMT".parse().unwrap();
        
        let email: Message = Message::new()
            .with_date(Some(date))
            .with_header(header::From(vec![Mailbox::new(Some("Каи".into()), "kayo@example.com".parse().unwrap())]))
            .with_header(header::To(vec!["Pony O.P. <pony@domain.tld>".parse().unwrap()]))
            .with_header(header::Subject("яңа ел белән!".into()))
            .with_body("\r\nHappy new year!");

        let (body, streamer) = email.streaming();

        handle.spawn(streamer.map_err(|_| ()));
        
        assert_eq!(core.run(body.concat2().map(|b| String::from(from_utf8(&b).unwrap()))).unwrap(),
                   concat!("Date: Tue, 15 Nov 1994 08:12:31 GMT\r\n",
                           "From: =?utf-8?b?0JrQsNC4?= <kayo@example.com>\r\n",
                           "To: Pony O.P. <pony@domain.tld>\r\n",
                           "Subject: =?utf-8?b?0Y/So9CwINC10Lsg0LHQtdC705nQvSE=?=\r\n",
                           "\r\n",
                           "Happy new year!"));
    }
}