use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

pub const CONTROL_PROTOCOL_VERSION: i64 = 1;
pub const CONTROL_MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const CONTROL_MAX_RESPONSE_BYTES: usize = 512 * 1024;
pub const CONTROL_MAX_REQUEST_ID_BYTES: usize = 256;
pub const CONTROL_MAX_METHOD_BYTES: usize = 128;
pub const CONTROL_MAX_PARAM_COUNT: usize = 32;
pub const CONTROL_MAX_PARAM_KEY_BYTES: usize = 128;
pub const CONTROL_MAX_PARAM_VALUE_BYTES: usize = 48 * 1024;

macro_rules! control_methods {
    ($(($variant:ident, $id:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum ControlMethod { $($variant),+ }

        impl ControlMethod {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $id),+ }
            }
        }

        impl FromStr for ControlMethod {
            type Err = ();
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value { $($id => Ok(Self::$variant),)+ _ => Err(()) }
            }
        }
    };
}

control_methods!(
    (Ping, "ping"),
    (Tree, "tree"),
    (Identify, "identify"),
    (Capabilities, "capabilities"),
    (WorkspaceCreate, "workspace.create"),
    (GroupCreate, "group.create"),
    (TabCreate, "tab.create"),
    (WorkspaceSelect, "workspace.select"),
    (GroupSelect, "group.select"),
    (TabSelect, "tab.select"),
    (WorkspaceRename, "workspace.rename"),
    (GroupRename, "group.rename"),
    (TabRename, "tab.rename"),
    (PaneRename, "pane.rename"),
    (WorkspaceMove, "workspace.move"),
    (GroupMove, "group.move"),
    (TabMove, "tab.move"),
    (PaneMove, "pane.move"),
    (WorkspaceClose, "workspace.close"),
    (GroupClose, "group.close"),
    (TabClose, "tab.close"),
    (PaneClose, "pane.close"),
    (PaneSplit, "pane.split"),
    (PaneFocus, "pane.focus"),
    (PaneSend, "pane.send"),
    (PaneSendKey, "pane.send_key"),
    (PaneReadScreen, "pane.read_screen"),
    (PaneNotify, "pane.notify"),
    (EventList, "event.list"),
    (AgentStart, "agent.start"),
    (AgentList, "agent.list"),
    (AgentGet, "agent.get"),
    (AgentUpdate, "agent.update"),
    (AgentFocus, "agent.focus"),
    (AgentResume, "agent.resume"),
    (TodoCreate, "todo.create"),
    (TodoList, "todo.list"),
    (TodoCheck, "todo.check"),
    (TodoReopen, "todo.reopen"),
    (TodoDelete, "todo.delete"),
    (TodoExport, "todo.export"),
    (SshProfileList, "ssh.profile.list"),
    (SshConnect, "ssh.connect"),
    (SshReconnect, "ssh.reconnect"),
);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ControlRequest {
    pub version: i64,
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

impl ControlRequest {
    #[must_use]
    pub fn method_id(&self) -> Option<ControlMethod> {
        self.method.parse().ok()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ControlError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ControlResponse {
    pub version: i64,
    pub id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ControlError>,
}

impl ControlResponse {
    #[must_use]
    pub fn success(id: impl Into<String>, result: Value) -> Self {
        Self {
            version: CONTROL_PROTOCOL_VERSION,
            id: id.into(),
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    #[must_use]
    pub fn failure(
        id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            version: CONTROL_PROTOCOL_VERSION,
            id: id.into(),
            ok: false,
            result: None,
            error: Some(ControlError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlCodecError {
    RequestTooLarge,
    ResponseTooLarge,
    MalformedRequest,
    InvalidParams,
    MalformedResponse,
    UnsupportedVersion(i64),
    InvalidEnvelope,
    InvalidResponse,
    IncompleteFrame,
}

impl ControlCodecError {
    #[must_use]
    pub const fn response_code(&self) -> &'static str {
        match self {
            Self::RequestTooLarge => "request_too_large",
            Self::ResponseTooLarge => "response_too_large",
            Self::InvalidParams => "invalid_params",
            Self::UnsupportedVersion(_) => "unsupported_version",
            Self::InvalidEnvelope | Self::InvalidResponse => "invalid_request",
            Self::MalformedRequest | Self::MalformedResponse | Self::IncompleteFrame => {
                "malformed_request"
            }
        }
    }
}

impl fmt::Display for ControlCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestTooLarge => f.write_str("request exceeds 64 KiB"),
            Self::ResponseTooLarge => f.write_str("response exceeds 512 KiB"),
            Self::MalformedRequest => f.write_str("request is not valid protocol JSON"),
            Self::InvalidParams => f.write_str("request parameters are invalid or exceed bounds"),
            Self::MalformedResponse => f.write_str("response is not valid protocol JSON"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported protocol version {version}")
            }
            Self::InvalidEnvelope => f.write_str("request id or method is invalid"),
            Self::InvalidResponse => f.write_str("response success/error fields are inconsistent"),
            Self::IncompleteFrame => f.write_str("connection ended before newline frame"),
        }
    }
}

impl std::error::Error for ControlCodecError {}

pub fn decode_control_request(data: &[u8]) -> Result<ControlRequest, ControlCodecError> {
    if data.len() > CONTROL_MAX_REQUEST_BYTES {
        return Err(ControlCodecError::RequestTooLarge);
    }
    let request: ControlRequest =
        serde_json::from_slice(data).map_err(|_| ControlCodecError::MalformedRequest)?;
    if request.version != CONTROL_PROTOCOL_VERSION {
        return Err(ControlCodecError::UnsupportedVersion(request.version));
    }
    if request.id.is_empty()
        || request.id.len() > CONTROL_MAX_REQUEST_ID_BYTES
        || request.method.is_empty()
        || request.method.len() > CONTROL_MAX_METHOD_BYTES
        || request.id.chars().any(char::is_control)
        || request.method.chars().any(char::is_control)
    {
        return Err(ControlCodecError::InvalidEnvelope);
    }
    validate_control_params(&request)?;
    Ok(request)
}

fn validate_control_params(request: &ControlRequest) -> Result<(), ControlCodecError> {
    if request.params.len() > CONTROL_MAX_PARAM_COUNT {
        return Err(ControlCodecError::InvalidParams);
    }
    for (key, value) in &request.params {
        let allows_control_value =
            request.method_id() == Some(ControlMethod::PaneSend) && key == "text";
        if key.is_empty()
            || key.len() > CONTROL_MAX_PARAM_KEY_BYTES
            || key.chars().any(char::is_control)
            || value.is_empty()
            || value.len() > CONTROL_MAX_PARAM_VALUE_BYTES
            || (!allows_control_value && value.chars().any(char::is_control))
        {
            return Err(ControlCodecError::InvalidParams);
        }
    }
    Ok(())
}

pub fn encode_control_request(request: &ControlRequest) -> Result<Vec<u8>, ControlCodecError> {
    let bytes = serde_json::to_vec(request).map_err(|_| ControlCodecError::MalformedRequest)?;
    decode_control_request(&bytes)?;
    Ok(bytes)
}

pub fn decode_control_response(data: &[u8]) -> Result<ControlResponse, ControlCodecError> {
    if data.len() > CONTROL_MAX_RESPONSE_BYTES {
        return Err(ControlCodecError::ResponseTooLarge);
    }
    let response: ControlResponse =
        serde_json::from_slice(data).map_err(|_| ControlCodecError::MalformedResponse)?;
    if response.version != CONTROL_PROTOCOL_VERSION {
        return Err(ControlCodecError::UnsupportedVersion(response.version));
    }
    if response.id.len() > CONTROL_MAX_REQUEST_ID_BYTES
        || response.ok == response.error.is_some()
        || (response.ok && response.result.is_none())
        || (!response.ok && response.result.is_some())
    {
        return Err(ControlCodecError::InvalidResponse);
    }
    Ok(response)
}

pub fn encode_control_response(response: &ControlResponse) -> Result<Vec<u8>, ControlCodecError> {
    let bytes = serde_json::to_vec(response).map_err(|_| ControlCodecError::MalformedResponse)?;
    if bytes.len() > CONTROL_MAX_RESPONSE_BYTES {
        return Err(ControlCodecError::ResponseTooLarge);
    }
    decode_control_response(&bytes)?;
    Ok(bytes)
}

pub struct LineFrameDecoder {
    maximum_bytes: usize,
    buffered: Vec<u8>,
}

impl LineFrameDecoder {
    #[must_use]
    pub fn request() -> Self {
        Self {
            maximum_bytes: CONTROL_MAX_REQUEST_BYTES,
            buffered: Vec::new(),
        }
    }

    #[must_use]
    pub fn response() -> Self {
        Self {
            maximum_bytes: CONTROL_MAX_RESPONSE_BYTES,
            buffered: Vec::new(),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, ControlCodecError> {
        self.buffered.extend_from_slice(bytes);
        let mut frames = Vec::new();
        while let Some(index) = self.buffered.iter().position(|byte| *byte == b'\n') {
            let payload_bytes = if index > 0 && self.buffered[index - 1] == b'\r' {
                index - 1
            } else {
                index
            };
            if payload_bytes > self.maximum_bytes {
                return Err(self.too_large());
            }
            let mut frame: Vec<u8> = self.buffered.drain(..=index).collect();
            frame.pop();
            if frame.last() == Some(&b'\r') {
                frame.pop();
            }
            frames.push(frame);
        }
        if self.buffered.len() > self.maximum_bytes
            && !(self.buffered.len() == self.maximum_bytes + 1
                && self.buffered.last() == Some(&b'\r'))
        {
            return Err(self.too_large());
        }
        Ok(frames)
    }

    pub fn finish(self) -> Result<(), ControlCodecError> {
        self.buffered
            .is_empty()
            .then_some(())
            .ok_or(ControlCodecError::IncompleteFrame)
    }

    fn too_large(&self) -> ControlCodecError {
        if self.maximum_bytes == CONTROL_MAX_REQUEST_BYTES {
            ControlCodecError::RequestTooLarge
        } else {
            ControlCodecError::ResponseTooLarge
        }
    }
}
