/// Utility functions
use crate::error::Error;
use nix::errno::Errno;
use nix::poll::{poll, PollFd, PollFlags};
use nix::unistd::read;
use std::os::unix::io::RawFd;
use std::time::Instant;

pub fn poll_retry(mut fds: &mut [PollFd], timeout: usize) -> Result<i32, Error> {
    let now = Instant::now();
    let mut remaining: i32 = timeout.try_into().unwrap();
    loop {
        let ret = poll(&mut fds, remaining);
        let elapsed = now.elapsed().as_millis();
        remaining = remaining
            .checked_sub(elapsed.try_into().unwrap())
            .unwrap_or(0);

        if remaining == 0 {
            return Err(Error::Timeout);
        }

        match ret {
            Ok(nfds) => return Ok(nfds),
            Err(Errno::EINTR) => (),
            Err(e) => return Err(e.into()),
        }
    }
}

pub fn read_timeout(fd: RawFd, mut buf: &mut [u8], timeout: usize) -> Result<(), Error> {
    let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];

    while !buf.is_empty() {
        poll_retry(&mut fds, timeout)?;

        if fds[0]
            .revents()
            .map_or(true, |e| !e.contains(PollFlags::POLLIN))
        {
            return Err(Error::Generic("POLLING poll flag missing".into()));
        }

        match read(fd, buf) {
            Ok(0) => break,
            Ok(n) => {
                let tmp = buf;
                buf = &mut tmp[n..];
            }
            Err(e) => return Err(e.into()),
        }
    }
    if !buf.is_empty() {
        Err(Error::MissingData)
    } else {
        Ok(())
    }
}
