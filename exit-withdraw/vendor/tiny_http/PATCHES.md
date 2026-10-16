# Local tiny_http changes

This is tiny_http 0.12.0, vendored to set read and write timeouts on accepted
client connections. The new `Server::http_with_timeout` constructor leaves the
listening socket blocking, so idle periods do not terminate the accept loop.

The application also enforces its own total deadline while reading JSON bodies.
The socket timeout bounds a stalled read inside tiny_http, including its early
body buffering before a request reaches the application.
