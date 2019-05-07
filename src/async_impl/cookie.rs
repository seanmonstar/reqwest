// FIXME: should this even be an option? the idea of running a session async seems a little iffy
use std::sync::{Arc, RwLock};
use std::marker::PhantomData;

use cookie::CookieStorage;
define_session!(super::Client, super::ClientBuilder, super::ClientBuilder::new());
