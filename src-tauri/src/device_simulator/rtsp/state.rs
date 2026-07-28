use std::collections::BTreeMap;

pub const MAX_RTSP_REQUEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtspMethod {
    Options,
    Describe,
    Setup,
    Play,
    FastPlay,
    GetParameter,
    Teardown,
    /// Any other verb. The legacy IPCRtsp server only branched on OPTIONS,
    /// SETUP, PLAY, FASTPLAY and GET_PARAMETER and answered every remaining
    /// request with its DESCRIBE SDP, so PAUSE/SET_PARAMETER/ANNOUNCE never
    /// broke a session. Keeping that fallback is required for parity.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtspRequest {
    pub method: RtspMethod,
    pub uri: String,
    pub cseq: u32,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtspSessionState {
    Initial,
    Ready,
    Playing,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtspDecision {
    Options,
    Describe,
    SetupTcpInterleaved { rtp_channel: u8, rtcp_channel: u8 },
    Play,
    FastPlay,
    KeepAlive,
    Teardown,
    Error { status: u16, reason: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtspSession {
    pub state: RtspSessionState,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtspParseError {
    pub code: &'static str,
    pub message: String,
}

impl RtspParseError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl RtspSession {
    pub fn new(session_id: String) -> Self {
        Self {
            state: RtspSessionState::Initial,
            session_id,
        }
    }

    pub fn handle(&mut self, request: &RtspRequest) -> RtspDecision {
        if self.state == RtspSessionState::Closed {
            return error(454, "Session Not Found");
        }
        match request.method {
            RtspMethod::Options => RtspDecision::Options,
            RtspMethod::Describe => RtspDecision::Describe,
            // Legacy answered every SETUP with the same 200, including the
            // extra track some platforms set up after PLAY. The service only
            // ever spawns one stream task, so a late SETUP cannot disturb the
            // video already in flight.
            RtspMethod::Setup => match parse_tcp_interleaved(request.headers.get("transport")) {
                Ok((rtp_channel, rtcp_channel)) => {
                    if self.state != RtspSessionState::Playing {
                        self.state = RtspSessionState::Ready;
                    }
                    RtspDecision::SetupTcpInterleaved {
                        rtp_channel,
                        rtcp_channel,
                    }
                }
                Err(()) => error(461, "Unsupported Transport"),
            },
            // The legacy server started streaming on the first PLAY it saw and
            // never tracked SETUP state, so clients that PLAY an aggregate URL
            // without a per-track SETUP still get video. Rejecting those with
            // 455 is a port regression, not legacy behaviour.
            RtspMethod::Play => {
                self.state = RtspSessionState::Playing;
                RtspDecision::Play
            }
            // A repeated FASTPLAY re-sent the SDP in the legacy server; only one
            // stream task is ever started, so answering again is safe.
            RtspMethod::FastPlay => {
                self.state = RtspSessionState::Playing;
                RtspDecision::FastPlay
            }
            // Legacy answered GET_PARAMETER with 200 in every state; platforms
            // use it as a pre-SETUP probe as well as a keep-alive.
            RtspMethod::GetParameter => RtspDecision::KeepAlive,
            RtspMethod::Teardown => {
                self.state = RtspSessionState::Closed;
                RtspDecision::Teardown
            }
            RtspMethod::Other => RtspDecision::Describe,
        }
    }
}

pub fn parse_rtsp_request(bytes: &[u8]) -> Result<RtspRequest, RtspParseError> {
    if bytes.is_empty() || bytes.len() > MAX_RTSP_REQUEST_BYTES {
        return Err(RtspParseError::new(
            "device_simulator.rtsp.request_size_invalid",
            "RTSP request is empty or exceeds 64 KiB",
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        RtspParseError::new(
            "device_simulator.rtsp.request_encoding_invalid",
            "RTSP request headers must be UTF-8/ASCII",
        )
    })?;
    let header_text = text
        .split_once("\r\n\r\n")
        .map(|(head, _)| head)
        .unwrap_or(text);
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let parts = request_line.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 3 || parts[2] != "RTSP/1.0" {
        return Err(RtspParseError::new(
            "device_simulator.rtsp.request_line_invalid",
            "RTSP request line must be '<method> <uri> RTSP/1.0'",
        ));
    }
    let method = match parts[0] {
        "OPTIONS" => RtspMethod::Options,
        "DESCRIBE" => RtspMethod::Describe,
        "SETUP" => RtspMethod::Setup,
        "PLAY" => RtspMethod::Play,
        "FASTPLAY" => RtspMethod::FastPlay,
        "GET_PARAMETER" => RtspMethod::GetParameter,
        "TEARDOWN" => RtspMethod::Teardown,
        // PAUSE, SET_PARAMETER, ANNOUNCE and anything else fall back to the
        // legacy SDP answer instead of tearing the connection down.
        _ => RtspMethod::Other,
    };
    if !parts[1].starts_with("rtsp://") && !parts[1].starts_with('/') && parts[1] != "*" {
        return Err(RtspParseError::new(
            "device_simulator.rtsp.uri_invalid",
            "RTSP URI must be absolute, origin-form, or '*'",
        ));
    }

    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or_else(|| {
            RtspParseError::new(
                "device_simulator.rtsp.header_invalid",
                format!("RTSP header has no colon: '{line}'"),
            )
        })?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(RtspParseError::new(
                "device_simulator.rtsp.header_invalid",
                "RTSP header name is invalid",
            ));
        }
        // Legacy read the first occurrence of each header and ignored the rest.
        // Rejecting duplicates dropped sessions from clients that repeat a
        // header, so keep the first value instead.
        headers
            .entry(name)
            .or_insert_with(|| value.trim().to_string());
    }
    let cseq = headers
        .get("cseq")
        .ok_or_else(|| {
            RtspParseError::new("device_simulator.rtsp.cseq_missing", "CSeq is required")
        })?
        .parse::<u32>()
        .map_err(|_| {
            RtspParseError::new("device_simulator.rtsp.cseq_invalid", "CSeq must be u32")
        })?;
    Ok(RtspRequest {
        method,
        uri: parts[1].to_string(),
        cseq,
        headers,
    })
}

pub fn build_rtsp_response(
    cseq: u32,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Vec<u8> {
    let mut response = format!("RTSP/1.0 {status} {reason}\r\nCSeq: {cseq}\r\n");
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    if !body.is_empty() {
        response.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    response.push_str("\r\n");
    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Declared body length of a request whose headers have already been read.
/// RTSP bodies are rare but GET_PARAMETER and SET_PARAMETER carry them; leaving
/// the bytes in the socket would desynchronise the next request.
pub fn declared_body_length(header_bytes: &[u8]) -> usize {
    let Ok(text) = std::str::from_utf8(header_bytes) else {
        return 0;
    };
    text.split("\r\n")
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0)
        .min(MAX_RTSP_REQUEST_BYTES)
}

fn parse_tcp_interleaved(value: Option<&String>) -> Result<(u8, u8), ()> {
    let value = value.ok_or(())?;
    if !value
        .split(';')
        .next()
        .is_some_and(|protocol| protocol.trim().eq_ignore_ascii_case("RTP/AVP/TCP"))
    {
        return Err(());
    }
    let channels = value
        .split(';')
        .find_map(|part| part.trim().strip_prefix("interleaved="))
        .ok_or(())?;
    let (rtp, rtcp) = channels.split_once('-').ok_or(())?;
    let rtp = rtp.parse::<u8>().map_err(|_| ())?;
    let rtcp = rtcp.parse::<u8>().map_err(|_| ())?;
    if rtp == rtcp {
        return Err(());
    }
    Ok((rtp, rtcp))
}

fn error(status: u16, reason: &'static str) -> RtspDecision {
    RtspDecision::Error { status, reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, extra: &str) -> RtspRequest {
        parse_rtsp_request(
            format!("{method} rtsp://192.0.2.2/media/video1 RTSP/1.0\r\nCSeq: 7\r\n{extra}\r\n")
                .as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn parses_bounded_requests_and_tolerates_unknown_methods_or_repeated_headers() {
        assert_eq!(request("OPTIONS", "").cseq, 7);
        assert_eq!(
            parse_rtsp_request(b"PAUSE * RTSP/1.0\r\nCSeq: 1\r\n\r\n")
                .unwrap()
                .method,
            RtspMethod::Other
        );
        // A repeated header keeps the first value instead of dropping the
        // session, matching the legacy parser.
        assert_eq!(
            parse_rtsp_request(b"OPTIONS * RTSP/1.0\r\nCSeq: 1\r\nCSeq: 2\r\n\r\n")
                .unwrap()
                .cseq,
            1
        );
        assert_eq!(
            declared_body_length(b"GET_PARAMETER * RTSP/1.0\r\nContent-Length: 12\r\n"),
            12
        );
        assert_eq!(
            declared_body_length(b"OPTIONS * RTSP/1.0\r\nCSeq: 1\r\n"),
            0
        );
    }

    #[test]
    fn enforces_tcp_only_and_valid_state_transitions() {
        let mut session = RtspSession::new("session-1".into());
        // Legacy started streaming on a bare PLAY, so this must not 455.
        assert_eq!(session.handle(&request("PLAY", "")), RtspDecision::Play);
        session.state = RtspSessionState::Initial;
        assert_eq!(
            session.handle(&request("PAUSE", "")),
            RtspDecision::Describe
        );
        assert_eq!(
            session.handle(&request(
                "SETUP",
                "Transport: RTP/AVP;unicast;client_port=5000-5001\r\n"
            )),
            error(461, "Unsupported Transport")
        );
        assert_eq!(
            session.handle(&request(
                "SETUP",
                "Transport: RTP/AVP/TCP;unicast;interleaved=0-1\r\n"
            )),
            RtspDecision::SetupTcpInterleaved {
                rtp_channel: 0,
                rtcp_channel: 1
            }
        );
        assert_eq!(session.state, RtspSessionState::Ready);
        assert_eq!(session.handle(&request("PLAY", "")), RtspDecision::Play);
        assert_eq!(session.state, RtspSessionState::Playing);
        assert_eq!(
            session.handle(&request("GET_PARAMETER", "")),
            RtspDecision::KeepAlive
        );
        assert_eq!(
            session.handle(&request("TEARDOWN", "")),
            RtspDecision::Teardown
        );
        assert_eq!(session.state, RtspSessionState::Closed);
    }

    #[test]
    fn response_has_exact_cseq_and_binary_body_length() {
        let response =
            build_rtsp_response(9, 200, "OK", &[("Content-Type", "application/sdp")], b"abc");
        let text = String::from_utf8(response).unwrap();
        assert!(text.contains("CSeq: 9\r\n"));
        assert!(text.contains("Content-Length: 3\r\n"));
        assert!(text.ends_with("\r\n\r\nabc"));
    }
}
