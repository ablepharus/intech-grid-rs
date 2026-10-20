//! Grid packet framing.
//!
//! A packet looks like this (all fields ASCII hex):
//!
//! ```text
//! SOH BRC LEN(4) ID(2) SESSION(2) SX(2) SY(2) DX(2) DY(2) ROT(1) PORTROT(1) MSGAGE(2) EOB
//!     STX <class:3> <instr:1> <payload...> ETX   (one or more bodies)
//! EOT <checksum:2> LF
//! ```
//!
//! `LEN` is the total packet length minus the trailing checksum and LF (3 bytes),
//! and the checksum is an XOR over every byte preceding those same 3 bytes.

use crate::error::{Error, Result};
use crate::proto::{brc, ctrl};

/// The broadcast header that prefixes every packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Rolling message id.
    pub id: u8,
    /// Session byte, constant for the lifetime of a connection.
    pub session: u8,
    /// Source position.
    pub sx: u8,
    pub sy: u8,
    /// Destination position. `0` means "global", i.e. every module.
    pub dx: u8,
    pub dy: u8,
    /// Module rotation (single hex digit).
    pub rot: u8,
    /// Port rotation (single hex digit).
    pub portrot: u8,
    /// Message age (hop count).
    pub msgage: u8,
}

impl Header {
    /// Header addressed at every module on the bus (`dx`/`dy` of 0).
    ///
    /// Note that the CONFIG class rejects globally addressed packets; use
    /// [`Header::this_module`] for anything that reads or writes configuration.
    pub fn global(id: u8, session: u8) -> Self {
        Header {
            id,
            session,
            sx: 0,
            sy: 0,
            dx: 0,
            dy: 0,
            rot: 0,
            portrot: 0,
            msgage: 0,
        }
    }

    /// Header addressed at the directly attached module.
    ///
    /// `grid_check_destination` in the firmware treats `dx == dy == 127`
    /// (`GRID_PARAMETER_DEFAULT_POSITION`) as "is me", which every class this
    /// crate uses accepts, including CONFIG.
    pub fn this_module(id: u8, session: u8) -> Self {
        Header {
            dx: crate::proto::DEFAULT_POSITION,
            dy: crate::proto::DEFAULT_POSITION,
            ..Header::global(id, session)
        }
    }
}

/// A single class body inside a packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    /// Class code, e.g. `class::CONFIG`.
    pub class: u16,
    /// Instruction nibble, e.g. `instr::EXECUTE`.
    pub instr: u8,
    /// Everything between the instruction nibble and ETX.
    pub payload: String,
}

impl Body {
    pub fn new(class: u16, instr: u8, payload: impl Into<String>) -> Self {
        Body {
            class,
            instr,
            payload: payload.into(),
        }
    }

    /// Read a hex field `(offset, length)` given relative to STX.
    ///
    /// Offsets in `grid_protocol.h` count STX as byte 0, the class code as
    /// bytes 1..4 and the instruction as byte 4, so payload starts at 5.
    pub fn field(&self, (offset, length): (usize, usize)) -> Result<u32> {
        let start = offset.checked_sub(5).ok_or_else(|| {
            Error::malformed(format!("field offset {offset} is inside the header"))
        })?;
        let end = start + length;
        let slice = self.payload.get(start..end).ok_or_else(|| {
            Error::malformed(format!(
                "field at {offset}+{length} is outside a {}-byte payload",
                self.payload.len()
            ))
        })?;
        u32::from_str_radix(slice, 16).map_err(|_| {
            Error::malformed(format!("field at {offset}+{length} is not hex: {slice:?}"))
        })
    }

    /// Read a hex field as a byte.
    pub fn field_u8(&self, spec: (usize, usize)) -> Result<u8> {
        let v = self.field(spec)?;
        u8::try_from(v)
            .map_err(|_| Error::malformed(format!("field value {v} does not fit in a byte")))
    }

    /// Everything from `offset` (relative to STX) to the end of the payload.
    pub fn tail(&self, offset: usize) -> Result<&str> {
        let start = offset.checked_sub(5).ok_or_else(|| {
            Error::malformed(format!("tail offset {offset} is inside the header"))
        })?;
        self.payload
            .get(start..)
            .ok_or_else(|| Error::malformed(format!("tail offset {offset} is past the payload")))
    }
}

/// A decoded packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub header: Header,
    pub bodies: Vec<Body>,
}

impl Frame {
    /// First body with the given class code.
    pub fn body(&self, class: u16) -> Option<&Body> {
        self.bodies.iter().find(|b| b.class == class)
    }
}

/// XOR checksum over `bytes`, which must already exclude the trailing
/// checksum digits and LF.
pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |acc, b| acc ^ b)
}

fn push_hex(out: &mut Vec<u8>, value: u32, digits: usize) {
    let s = format!("{value:0digits$x}", digits = digits);
    out.extend_from_slice(s.as_bytes());
}

/// Encode a packet, filling in `LEN` and the checksum.
pub fn encode(header: &Header, bodies: &[Body]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(64);
    out.push(ctrl::SOH);
    out.push(ctrl::BRC);
    out.extend_from_slice(b"0000"); // LEN, patched below
    push_hex(&mut out, header.id as u32, brc::ID.1);
    push_hex(&mut out, header.session as u32, brc::SESSION.1);
    push_hex(&mut out, header.sx as u32, brc::SX.1);
    push_hex(&mut out, header.sy as u32, brc::SY.1);
    push_hex(&mut out, header.dx as u32, brc::DX.1);
    push_hex(&mut out, header.dy as u32, brc::DY.1);
    push_hex(&mut out, header.rot as u32, brc::ROT.1);
    push_hex(&mut out, header.portrot as u32, brc::PORTROT.1);
    push_hex(&mut out, header.msgage as u32, brc::MSGAGE.1);
    out.push(ctrl::EOB);
    debug_assert_eq!(out.len(), brc::LEN_TOTAL);

    for body in bodies {
        out.push(ctrl::STX);
        push_hex(&mut out, body.class as u32, 3);
        push_hex(&mut out, body.instr as u32, 1);
        out.extend_from_slice(body.payload.as_bytes());
        out.push(ctrl::ETX);
    }

    out.push(ctrl::EOT);
    out.extend_from_slice(b"00"); // checksum, patched below
    out.push(ctrl::LF);

    let total = out.len();
    let len_field = total - 3;
    if len_field > 0xffff {
        return Err(Error::malformed(format!(
            "packet of {total} bytes is too long to encode"
        )));
    }
    let len_digits = format!("{len_field:04x}");
    out[brc::LEN.0..brc::LEN.0 + brc::LEN.1].copy_from_slice(len_digits.as_bytes());

    let sum = checksum(&out[..total - 3]);
    let sum_digits = format!("{sum:02x}");
    out[total - 3..total - 1].copy_from_slice(sum_digits.as_bytes());

    Ok(out)
}

fn hex_at(bytes: &[u8], (offset, length): (usize, usize)) -> Result<u8> {
    let slice = bytes.get(offset..offset + length).ok_or_else(|| {
        Error::malformed(format!("header field at {offset}+{length} is out of range"))
    })?;
    let text = std::str::from_utf8(slice)
        .map_err(|_| Error::malformed(format!("header field at {offset}+{length} is not ASCII")))?;
    u8::from_str_radix(text, 16).map_err(|_| {
        Error::malformed(format!(
            "header field at {offset}+{length} is not hex: {text:?}"
        ))
    })
}

/// Decode a single packet, validating its length field and checksum.
pub fn parse(bytes: &[u8]) -> Result<Frame> {
    if bytes.len() < brc::LEN_TOTAL + 4 {
        return Err(Error::malformed(format!(
            "packet of {} bytes is too short",
            bytes.len()
        )));
    }
    if bytes[0] != ctrl::SOH {
        return Err(Error::malformed(format!(
            "expected SOH, found 0x{:02x}",
            bytes[0]
        )));
    }
    if bytes[1] != ctrl::BRC && bytes[1] != ctrl::DCT {
        return Err(Error::malformed(format!(
            "expected BRC or DCT, found 0x{:02x}",
            bytes[1]
        )));
    }
    if *bytes.last().unwrap() != ctrl::LF {
        return Err(Error::malformed("packet does not end with LF"));
    }
    if bytes[brc::LEN_TOTAL - 1] != ctrl::EOB {
        return Err(Error::malformed("header is not terminated by EOB"));
    }

    let stated_len = {
        let slice = &bytes[brc::LEN.0..brc::LEN.0 + brc::LEN.1];
        let text = std::str::from_utf8(slice).map_err(|_| Error::malformed("LEN is not ASCII"))?;
        usize::from_str_radix(text, 16)
            .map_err(|_| Error::malformed(format!("LEN is not hex: {text:?}")))?
    };
    if stated_len != bytes.len() - 3 {
        return Err(Error::malformed(format!(
            "LEN says {stated_len} but packet body is {} bytes",
            bytes.len() - 3
        )));
    }

    let computed = checksum(&bytes[..bytes.len() - 3]);
    let stated = {
        let slice = &bytes[bytes.len() - 3..bytes.len() - 1];
        let text =
            std::str::from_utf8(slice).map_err(|_| Error::malformed("checksum is not ASCII"))?;
        u8::from_str_radix(text, 16)
            .map_err(|_| Error::malformed(format!("checksum is not hex: {text:?}")))?
    };
    if computed != stated {
        return Err(Error::Checksum { computed, stated });
    }

    let header = Header {
        id: hex_at(bytes, brc::ID)?,
        session: hex_at(bytes, brc::SESSION)?,
        sx: hex_at(bytes, brc::SX)?,
        sy: hex_at(bytes, brc::SY)?,
        dx: hex_at(bytes, brc::DX)?,
        dy: hex_at(bytes, brc::DY)?,
        rot: hex_at(bytes, brc::ROT)?,
        portrot: hex_at(bytes, brc::PORTROT)?,
        msgage: hex_at(bytes, brc::MSGAGE)?,
    };

    // Bodies live between EOB and EOT.
    let eot = bytes
        .iter()
        .rposition(|&b| b == ctrl::EOT)
        .ok_or_else(|| Error::malformed("packet has no EOT"))?;
    let mut bodies = Vec::new();
    let mut cursor = brc::LEN_TOTAL;
    while cursor < eot {
        match bytes[cursor] {
            ctrl::STX => {
                let end = bytes[cursor..eot]
                    .iter()
                    .position(|&b| b == ctrl::ETX)
                    .map(|p| cursor + p)
                    .ok_or_else(|| Error::malformed("body is not terminated by ETX"))?;
                let inner = &bytes[cursor + 1..end];
                if inner.len() < 4 {
                    return Err(Error::malformed("body is shorter than its class code"));
                }
                let text = std::str::from_utf8(inner)
                    .map_err(|_| Error::malformed("body is not ASCII"))?;
                let class = u16::from_str_radix(&text[0..3], 16).map_err(|_| {
                    Error::malformed(format!("class code is not hex: {:?}", &text[0..3]))
                })?;
                let instr = u8::from_str_radix(&text[3..4], 16).map_err(|_| {
                    Error::malformed(format!("instruction is not hex: {:?}", &text[3..4]))
                })?;
                bodies.push(Body::new(class, instr, &text[4..]));
                cursor = end + 1;
            }
            // Tolerate padding between bodies.
            ctrl::NUL | ctrl::LF => cursor += 1,
            other => {
                return Err(Error::malformed(format!(
                    "unexpected byte 0x{other:02x} where a body should start"
                )))
            }
        }
    }

    Ok(Frame { header, bodies })
}

/// Split a byte stream into packets, returning the packets and any trailing
/// partial packet that should be kept for the next read.
pub fn split_packets(buf: &[u8]) -> (Vec<&[u8]>, &[u8]) {
    let mut packets = Vec::new();
    let mut start = 0;
    for (i, &b) in buf.iter().enumerate() {
        if b == ctrl::LF {
            packets.push(&buf[start..=i]);
            start = i + 1;
        }
    }
    (packets, &buf[start..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{class, heartbeat_fields, instr};

    /// Two heartbeat packets captured from an EN16 RevD on firmware 1.5.1.
    const CAPTURED: [&[u8]; 2] = [
        b"\x01\x0f00344b0d7f7f00000000\x17\x02010e01c10105010057\x03\x02030d00\x03\x044f\n",
        b"\x01\x0f0034550d7f7f00000000\x17\x02010e01c10105010057\x03\x02030d00\x03\x0419\n",
    ];

    #[test]
    fn parses_captured_hardware_packets() {
        for raw in CAPTURED {
            let frame = parse(raw).expect("captured packet should parse");
            assert_eq!(frame.header.session, 0x0d);
            assert_eq!(frame.header.sx, 127, "module reports its default position");
            assert_eq!(frame.header.sy, 127);
            assert_eq!(frame.header.dx, 0, "addressed globally");
            assert_eq!(frame.bodies.len(), 2);

            let hb = frame.body(class::HEARTBEAT).expect("heartbeat body");
            assert_eq!(hb.instr, instr::EXECUTE);
            assert_eq!(hb.field_u8(heartbeat_fields::HWCFG).unwrap(), 0xc1);
            assert_eq!(hb.field_u8(heartbeat_fields::VERSION_MAJOR).unwrap(), 1);
            assert_eq!(hb.field_u8(heartbeat_fields::VERSION_MINOR).unwrap(), 5);
            assert_eq!(hb.field_u8(heartbeat_fields::VERSION_PATCH).unwrap(), 1);

            let page = frame.body(class::PAGEACTIVE).expect("pageactive body");
            assert_eq!(page.instr, instr::REPORT);
        }
    }

    #[test]
    fn reencodes_captured_packets_byte_for_byte() {
        for raw in CAPTURED {
            let frame = parse(raw).unwrap();
            let encoded = encode(&frame.header, &frame.bodies).unwrap();
            assert_eq!(
                encoded, raw,
                "re-encoding a captured packet must reproduce it exactly"
            );
        }
    }

    #[test]
    fn checksum_matches_firmware_xor() {
        // grid_frame_calculate_checksum_packet() XORs everything but the last 3 bytes.
        let raw = CAPTURED[0];
        assert_eq!(checksum(&raw[..raw.len() - 3]), 0x4f);
    }

    #[test]
    fn rejects_corrupted_checksum() {
        let mut raw = CAPTURED[0].to_vec();
        let last = raw.len() - 2;
        raw[last] = b'0';
        match parse(&raw) {
            Err(Error::Checksum { .. }) => {}
            other => panic!("expected a checksum error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_wrong_length_field() {
        let mut raw = CAPTURED[0].to_vec();
        raw[2] = b'9'; // LEN 9034
        assert!(matches!(parse(&raw), Err(Error::Malformed(_))));
    }

    #[test]
    fn this_module_is_addressed_at_the_default_position() {
        // The firmware only treats 127,127 as "me", and CONFIG accepts
        // nothing else over USB.
        let h = Header::this_module(1, 0x42);
        assert_eq!((h.dx, h.dy), (127, 127));
        let packet = encode(
            &h,
            &[Body::new(
                class::CONFIG,
                instr::FETCH,
                "010501000002 0000".replace(' ', ""),
            )],
        )
        .unwrap();
        let parsed = parse(&packet).unwrap();
        assert_eq!((parsed.header.dx, parsed.header.dy), (127, 127));
    }

    #[test]
    fn encodes_len_as_total_minus_three() {
        let header = Header::global(0x01, 0x42);
        let bodies = [Body::new(class::IMMEDIATE, instr::EXECUTE, "0005abcde")];
        let packet = encode(&header, &bodies).unwrap();
        let stated =
            usize::from_str_radix(std::str::from_utf8(&packet[2..6]).unwrap(), 16).unwrap();
        assert_eq!(stated, packet.len() - 3);
        // And it round-trips.
        let frame = parse(&packet).unwrap();
        assert_eq!(frame.bodies, bodies);
    }

    #[test]
    fn splits_stream_into_packets_and_keeps_remainder() {
        let mut stream = CAPTURED[0].to_vec();
        stream.extend_from_slice(CAPTURED[1]);
        stream.extend_from_slice(b"\x01\x0fpartial");
        let (packets, rest) = split_packets(&stream);
        assert_eq!(packets.len(), 2);
        assert_eq!(rest, b"\x01\x0fpartial");
        for p in packets {
            parse(p).unwrap();
        }
    }
}
