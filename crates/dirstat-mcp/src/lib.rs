mod clients;
mod protocol;
mod tools;

use std::io::{BufReader, Write};

pub use clients::{
    Client, ClientStatus, Locations, install, install_in, snippet, status, status_in, uninstall, uninstall_in,
};
pub use protocol::{LATEST_PROTOCOL_VERSION, PROTOCOL_VERSIONS, Server};
pub use tools::Options;

pub fn serve(options: Options) -> std::io::Result<()> {
    let mut server = Server::new(options);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let result = server.run(BufReader::new(stdin.lock()), stdout.lock());
    let _ = std::io::stderr().flush();
    result
}
