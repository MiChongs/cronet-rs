use std::{
    io::{self, Read, Write},
    net::{IpAddr, TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use hickory_proto::{
    op::Message,
    rr::{
        Name, RData, Record, RecordType,
        rdata::{
            A, AAAA, HTTPS, SVCB,
            svcb::{Alpn, EchConfigList, SvcParamKey, SvcParamValue},
        },
    },
};

pub(crate) type Resolver = dyn Fn(&[u8]) -> io::Result<Vec<u8>> + Send + Sync;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EchOptions {
    pub(crate) server_name: String,
    pub(crate) query_server_name: String,
    pub(crate) config_list: Vec<u8>,
    pub(crate) quic: bool,
}

pub(crate) fn with_ech(resolver: Arc<Resolver>, options: EchOptions) -> Arc<Resolver> {
    Arc::new(move |query| resolve_with_ech(&*resolver, &options, query))
}

pub(crate) fn with_server_redirect(
    resolver: Arc<Resolver>,
    server_name: String,
    server_address: String,
) -> Arc<Resolver> {
    Arc::new(move |query| {
        resolve_with_server_redirect(&*resolver, &server_name, &server_address, query)
    })
}

fn resolve_with_server_redirect(
    resolver: &Resolver,
    server_name: &str,
    server_address: &str,
    query_wire: &[u8],
) -> io::Result<Vec<u8>> {
    let request = Message::from_vec(query_wire).map_err(invalid_dns)?;
    let Some(question) = request.queries.first() else {
        return resolver(query_wire);
    };
    if !matches!(question.query_type(), RecordType::A | RecordType::AAAA)
        || !same_dns_name(&question.name().to_ascii(), server_name)
    {
        return resolver(query_wire);
    }

    if let Ok(address) = server_address.trim_matches(['[', ']']).parse::<IpAddr>() {
        let mut response = request.clone().into_response();
        response.answers.clear();
        response.authorities.clear();
        response.additionals.clear();
        let answer = match (question.query_type(), address) {
            (RecordType::A, IpAddr::V4(address)) => Some(RData::A(A(address))),
            (RecordType::AAAA, IpAddr::V6(address)) => Some(RData::AAAA(AAAA(address))),
            _ => None,
        };
        if let Some(answer) = answer {
            response.add_answer(Record::from_rdata(question.name().clone(), 300, answer));
        }
        return response.to_vec().map_err(invalid_dns);
    }

    let mut redirected = request.clone();
    redirected.queries[0].set_name(Name::from_ascii(server_address).map_err(invalid_dns)?);
    let response_wire = resolver(&redirected.to_vec().map_err(invalid_dns)?)?;
    let mut response = Message::from_vec(&response_wire).map_err(invalid_dns)?;
    response.queries = request.queries;
    for answer in &mut response.answers {
        if matches!(answer.data, RData::A(_) | RData::AAAA(_))
            && same_dns_name(&answer.name.to_ascii(), server_address)
        {
            answer.name = Name::from_ascii(server_name).map_err(invalid_dns)?;
        }
    }
    response.to_vec().map_err(invalid_dns)
}

fn resolve_with_ech(
    resolver: &Resolver,
    options: &EchOptions,
    query_wire: &[u8],
) -> io::Result<Vec<u8>> {
    let request = Message::from_vec(query_wire).map_err(invalid_dns)?;
    let Some(question) = request.queries.first() else {
        return resolver(query_wire);
    };
    let question_name = question.name().to_ascii();
    if question.query_type() != RecordType::HTTPS
        || !matches_server_name(&question_name, &options.server_name)
    {
        return resolver(query_wire);
    }

    if !options.config_list.is_empty() {
        return inject_ech_config(&request, &options.config_list, options.quic)?
            .to_vec()
            .map_err(invalid_dns);
    }

    let query_server_name = if options.query_server_name.is_empty() {
        &options.server_name
    } else {
        &options.query_server_name
    };
    let mut response = if !same_dns_name(query_server_name, &options.server_name) {
        let mut redirected = request.clone();
        let rewritten =
            rewrite_https_query_name(&question_name, &options.server_name, query_server_name);
        redirected.queries[0].set_name(Name::from_ascii(rewritten).map_err(invalid_dns)?);
        let wire = redirected.to_vec().map_err(invalid_dns)?;
        let response_wire = resolver(&wire)?;
        let mut response = Message::from_vec(&response_wire).map_err(invalid_dns)?;
        response.queries = request.queries.clone();
        rewrite_https_answer_names(&mut response, query_server_name, &options.server_name)?;
        response
    } else {
        Message::from_vec(&resolver(query_wire)?).map_err(invalid_dns)?
    };
    filter_ip_hints(&mut response);
    response.to_vec().map_err(invalid_dns)
}

fn inject_ech_config(request: &Message, ech_config: &[u8], quic: bool) -> io::Result<Message> {
    let Some(question) = request.queries.first() else {
        return Ok(request.clone().into_response());
    };
    let query_name = question.name().clone();
    let query_ascii = query_name.to_ascii();
    let target_ascii = query_ascii
        .strip_prefix('_')
        .and_then(|name| name.split_once("._https."))
        .map_or(query_ascii.as_str(), |(_, target)| target);
    let target_name = Name::from_ascii(target_ascii).map_err(invalid_dns)?;
    let mut params = Vec::with_capacity(3);
    if let Some(port) = parse_https_service_port(&query_ascii) {
        params.push((SvcParamKey::Port, SvcParamValue::Port(port)));
    }
    params.push((
        SvcParamKey::Alpn,
        SvcParamValue::Alpn(Alpn(vec![if quic { "h3" } else { "h2" }.to_owned()])),
    ));
    params.push((
        SvcParamKey::EchConfigList,
        SvcParamValue::EchConfigList(EchConfigList(ech_config.to_vec())),
    ));
    params.sort_by_key(|(key, _)| u16::from(*key));

    let mut response = request.clone().into_response();
    response.answers.clear();
    response.authorities.clear();
    response.additionals.clear();
    response.add_answer(Record::from_rdata(
        query_name,
        300,
        RData::HTTPS(HTTPS(SVCB::new(1, target_name, params))),
    ));
    Ok(response)
}

fn matches_server_name(query_name: &str, server_name: &str) -> bool {
    if same_dns_name(query_name, server_name) {
        return true;
    }
    let query_name = query_name.trim_end_matches('.');
    query_name
        .strip_prefix('_')
        .and_then(|name| name.split_once("._https."))
        .is_some_and(|(_, target)| same_dns_name(target, server_name))
}

fn same_dns_name(left: &str, right: &str) -> bool {
    left.trim_end_matches('.')
        .eq_ignore_ascii_case(right.trim_end_matches('.'))
}

fn rewrite_https_query_name(query_name: &str, from_server: &str, to_server: &str) -> String {
    let query_name = query_name.trim_end_matches('.');
    let from_server = from_server.trim_end_matches('.');
    let to_server = to_server.trim_end_matches('.');
    if let Some((prefix, target)) = query_name
        .strip_prefix('_')
        .and_then(|name| name.split_once("._https."))
        && target.eq_ignore_ascii_case(from_server)
    {
        return format!("_{prefix}._https.{to_server}.");
    }
    if query_name.eq_ignore_ascii_case(from_server) {
        return format!("{to_server}.");
    }
    format!("{query_name}.")
}

fn rewrite_https_answer_names(
    response: &mut Message,
    from_server: &str,
    to_server: &str,
) -> io::Result<()> {
    for answer in &mut response.answers {
        let RData::HTTPS(HTTPS(svcb)) = &mut answer.data else {
            continue;
        };
        let rewritten_owner =
            rewrite_https_query_name(&answer.name.to_ascii(), from_server, to_server);
        if !same_dns_name(&rewritten_owner, &answer.name.to_ascii()) {
            answer.name = Name::from_ascii(rewritten_owner).map_err(invalid_dns)?;
        }
        if same_dns_name(&svcb.target_name.to_ascii(), from_server) {
            svcb.target_name = Name::from_ascii(to_server).map_err(invalid_dns)?;
        }
    }
    Ok(())
}

fn filter_ip_hints(response: &mut Message) {
    for answer in &mut response.answers {
        let RData::HTTPS(HTTPS(svcb)) = &mut answer.data else {
            continue;
        };
        svcb.svc_params
            .retain(|(key, _)| !matches!(key, SvcParamKey::Ipv4Hint | SvcParamKey::Ipv6Hint));
    }
}

fn parse_https_service_port(query_name: &str) -> Option<u16> {
    let prefix = query_name
        .trim_end_matches('.')
        .strip_prefix('_')?
        .split_once("._https.")?
        .0;
    prefix.parse().ok()
}

fn invalid_dns(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

pub(crate) struct ShutdownGuard(pub(crate) Arc<AtomicBool>);

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(crate) fn tcp_proxy(
    resolver: Arc<Resolver>,
    shutdown: Arc<AtomicBool>,
) -> io::Result<TcpStream> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let cronet = TcpStream::connect(address)?;
    let (proxy, _) = listener.accept()?;
    proxy.set_read_timeout(Some(Duration::from_millis(250)))?;
    proxy.set_write_timeout(Some(Duration::from_secs(5)))?;
    thread::Builder::new()
        .name("cronet-dns-tcp".into())
        .spawn(move || serve_tcp(proxy, &*resolver, &shutdown))?;
    Ok(cronet)
}

pub(crate) fn udp_proxy(
    resolver: Arc<Resolver>,
    shutdown: Arc<AtomicBool>,
) -> io::Result<UdpSocket> {
    let cronet = UdpSocket::bind(("127.0.0.1", 0))?;
    let proxy = UdpSocket::bind(("127.0.0.1", 0))?;
    cronet.connect(proxy.local_addr()?)?;
    proxy.connect(cronet.local_addr()?)?;
    proxy.set_read_timeout(Some(Duration::from_millis(250)))?;
    proxy.set_write_timeout(Some(Duration::from_secs(5)))?;
    thread::Builder::new()
        .name("cronet-dns-udp".into())
        .spawn(move || serve_udp(proxy, &*resolver, &shutdown))?;
    Ok(cronet)
}

fn serve_tcp(mut stream: TcpStream, resolver: &Resolver, shutdown: &AtomicBool) {
    while !shutdown.load(Ordering::Acquire) {
        let mut length = [0_u8; 2];
        match read_exact_interruptible(&mut stream, &mut length, shutdown) {
            Ok(()) => {}
            Err(error) if is_shutdown_io(&error) => continue,
            Err(_) => return,
        }
        let length = usize::from(u16::from_be_bytes(length));
        let mut query = vec![0_u8; length];
        if read_exact_interruptible(&mut stream, &mut query, shutdown).is_err() {
            return;
        }
        let Ok(response) = resolver(&query) else {
            continue;
        };
        let Ok(response_length) = u16::try_from(response.len()) else {
            continue;
        };
        if stream.write_all(&response_length.to_be_bytes()).is_err()
            || stream.write_all(&response).is_err()
        {
            return;
        }
    }
}

fn serve_udp(socket: UdpSocket, resolver: &Resolver, shutdown: &AtomicBool) {
    let mut query = vec![0_u8; u16::MAX as usize];
    while !shutdown.load(Ordering::Acquire) {
        let length = match socket.recv(&mut query) {
            Ok(length) => length,
            Err(error) if is_shutdown_io(&error) => continue,
            Err(_) => return,
        };
        let Ok(response) = resolver(&query[..length]) else {
            continue;
        };
        if socket.send(&response).is_err() {
            return;
        }
    }
}

fn read_exact_interruptible(
    stream: &mut TcpStream,
    mut output: &mut [u8],
    shutdown: &AtomicBool,
) -> io::Result<()> {
    while !output.is_empty() {
        if shutdown.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "DNS proxy stopped",
            ));
        }
        match stream.read(output) {
            Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            Ok(count) => output = &mut output[count..],
            Err(error) if is_shutdown_io(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn is_shutdown_io(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        sync::{Arc, atomic::AtomicBool},
    };

    use hickory_proto::{
        op::{Message, Query},
        rr::{
            Name, RData, Record, RecordType,
            rdata::{
                A, HTTPS, SVCB,
                svcb::{IpHint, SvcParamKey, SvcParamValue},
            },
        },
    };

    use super::{EchOptions, Resolver, tcp_proxy, udp_proxy, with_ech, with_server_redirect};

    fn echo_resolver() -> Arc<Resolver> {
        Arc::new(|query: &[u8]| Ok(query.to_vec()))
    }

    fn https_query(name: &str) -> Vec<u8> {
        let mut message = Message::query();
        message.add_query(Query::query(
            Name::from_ascii(name).expect("query name"),
            RecordType::HTTPS,
        ));
        message.to_vec().expect("encode query")
    }

    #[test]
    fn proxies_length_prefixed_tcp_dns() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut socket = tcp_proxy(echo_resolver(), shutdown).expect("create TCP DNS proxy");
        socket.write_all(&[0, 3, 1, 2, 3]).expect("write query");
        let mut response = [0_u8; 5];
        socket.read_exact(&mut response).expect("read response");
        assert_eq!(response, [0, 3, 1, 2, 3]);
    }

    #[test]
    fn proxies_udp_dns_datagrams() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let socket = udp_proxy(echo_resolver(), shutdown).expect("create UDP DNS proxy");
        socket.send(&[1, 2, 3]).expect("write query");
        let mut response = [0_u8; 3];
        socket.recv(&mut response).expect("read response");
        assert_eq!(response, [1, 2, 3]);
    }

    #[test]
    fn injects_fixed_ech_config_with_protocol_and_service_port() {
        let resolver = with_ech(
            Arc::new(|_| panic!("fixed ECH must not call the upstream resolver")),
            EchOptions {
                server_name: "proxy.example".into(),
                query_server_name: "proxy.example".into(),
                config_list: vec![1, 2, 3, 4],
                quic: true,
            },
        );
        let response =
            resolver(&https_query("_8443._https.proxy.example.")).expect("resolve fixed ECH");
        let response = Message::from_vec(&response).expect("decode response");
        let RData::HTTPS(HTTPS(svcb)) = &response.answers[0].data else {
            panic!("expected HTTPS answer");
        };
        assert_eq!(svcb.target_name.to_ascii(), "proxy.example.");
        assert_eq!(
            svcb.svc_params
                .iter()
                .map(|(key, _)| *key)
                .collect::<Vec<_>>(),
            [
                SvcParamKey::Alpn,
                SvcParamKey::Port,
                SvcParamKey::EchConfigList
            ]
        );
        assert!(matches!(
            &svcb.svc_params[2].1,
            SvcParamValue::EchConfigList(config) if config.0 == [1, 2, 3, 4]
        ));
    }

    #[test]
    fn redirects_https_query_rewrites_names_and_filters_ip_hints() {
        let upstream: Arc<Resolver> = Arc::new(|wire| {
            let request = Message::from_vec(wire).expect("decode redirected query");
            assert_eq!(request.queries[0].name().to_ascii(), "ech.example.");
            let owner = request.queries[0].name().clone();
            let params = vec![(
                SvcParamKey::Ipv4Hint,
                SvcParamValue::Ipv4Hint(IpHint(vec![A::new(192, 0, 2, 1)])),
            )];
            let mut response = request.into_response();
            response.add_answer(Record::from_rdata(
                owner.clone(),
                300,
                RData::HTTPS(HTTPS(SVCB::new(1, owner, params))),
            ));
            response.to_vec().map_err(super::invalid_dns)
        });
        let resolver = with_ech(
            upstream,
            EchOptions {
                server_name: "proxy.example".into(),
                query_server_name: "ech.example".into(),
                config_list: Vec::new(),
                quic: false,
            },
        );
        let response =
            Message::from_vec(&resolver(&https_query("proxy.example.")).expect("resolve"))
                .expect("decode response");
        assert_eq!(response.queries[0].name().to_ascii(), "proxy.example.");
        let answer = &response.answers[0];
        assert_eq!(answer.name.to_ascii(), "proxy.example.");
        let RData::HTTPS(HTTPS(svcb)) = &answer.data else {
            panic!("expected HTTPS answer");
        };
        assert_eq!(svcb.target_name.to_ascii(), "proxy.example.");
        assert!(svcb.svc_params.is_empty());
    }

    #[test]
    fn synthesizes_server_address_for_tls_name() {
        let resolver = with_server_redirect(
            Arc::new(|_| panic!("IP redirect must not call the upstream resolver")),
            "front.example".into(),
            "192.0.2.7".into(),
        );
        let mut query = Message::query();
        query.add_query(Query::query(
            Name::from_ascii("front.example.").expect("name"),
            RecordType::A,
        ));
        let response = resolver(&query.to_vec().expect("encode"))
            .and_then(|wire| Message::from_vec(&wire).map_err(super::invalid_dns))
            .expect("resolve");
        assert_eq!(response.queries[0].name().to_ascii(), "front.example.");
        assert!(matches!(
            response.answers[0].data,
            RData::A(A(address)) if address.octets() == [192, 0, 2, 7]
        ));
    }
}
