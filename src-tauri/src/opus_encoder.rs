// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Bulbul contributors

//! Pure-Rust Ogg Opus Audio Encoder.
//!
//! Encodes 16 kHz Mono PCM audio samples (i16) into an RFC 7845 compliant Ogg Opus container
//! (`audio/ogg; codecs=opus`).
//!
//! Features:
//! - Pure Rust: Zero C/FFI dependencies, builds cleanly on Windows MSVC and Android NDK (aarch64).
//! - RFC 7845 Compliant: Outputs valid `OpusHead` and `OpusTags` headers inside Ogg pages.
//! - Granule math: RFC 7845 specifies granule positions in units of 48 kHz samples (3x 16 kHz).
//! - High Speech Efficiency: ~24-32 kbps VBR mono compresses speech by ~85-90% compared to WAV.

use anyhow::{anyhow, Result};
use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use rusty_opus::{Application, OpusEncoder};
use std::io::Cursor;

/// Target sample rate for speech dictation.
pub const OPUS_SAMPLE_RATE: i32 = 16_000;
/// Channels: 1 (Mono).
pub const OPUS_CHANNELS: usize = 1;
/// 20ms frame size at 16 kHz = 320 samples.
pub const OPUS_FRAME_SIZE: usize = 320;
/// RFC 7845 specifies granule positions are normalized to 48 kHz.
/// At 16 kHz input, 1 sample = 3 granule units (48000 / 16000 = 3).
const GRANULE_RATE_MULTIPLIER: u64 = 3;

/// Encodes 16 kHz mono i16 PCM samples into a self-contained RFC 7845 Ogg Opus byte buffer.
pub fn encode_pcm_to_ogg_opus(samples: &[i16]) -> Result<Vec<u8>> {
    let mut encoder = OpusEncoder::new(OPUS_SAMPLE_RATE, OPUS_CHANNELS, Application::Voip)
        .map_err(|e| anyhow!("Failed to initialize OpusEncoder: {e:?}"))?;

    let mut out_buf = Cursor::new(Vec::with_capacity(samples.len() / 6));
    let serial_number = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(0x42554C42))
        ^ 0xBEEF; // pseudo-random serial

    let mut ogg_writer = PacketWriter::new(&mut out_buf);

    // 1. Write OpusHead header packet (must be first packet on its own page)
    let head_packet = build_opus_head(OPUS_CHANNELS as u8, OPUS_SAMPLE_RATE as u32);
    ogg_writer
        .write_packet(head_packet.into_boxed_slice(), serial_number, PacketWriteEndInfo::EndPage, 0)
        .map_err(|e| anyhow!("Failed to write OpusHead packet: {e:?}"))?;

    // 2. Write OpusTags header packet (must be second packet on its own page)
    let tags_packet = build_opus_tags("bulbul");
    ogg_writer
        .write_packet(tags_packet.into_boxed_slice(), serial_number, PacketWriteEndInfo::EndPage, 0)
        .map_err(|e| anyhow!("Failed to write OpusTags packet: {e:?}"))?;

    // 3. Encode PCM frames in chunks of 320 samples (20 ms at 16 kHz)
    if samples.is_empty() {
        return Ok(out_buf.into_inner());
    }

    let mut packet_buf = vec![0u8; 1275]; // Max Opus packet size
    let mut granule_pos: u64 = 0;

    let chunks: Vec<&[i16]> = samples.chunks(OPUS_FRAME_SIZE).collect();
    let num_chunks = chunks.len();

    // Reusable buffer for f32 samples since rusty_opus::OpusEncoder expects &[f32]
    let mut f32_frame = vec![0.0f32; OPUS_FRAME_SIZE];

    for (idx, chunk) in chunks.into_iter().enumerate() {
        let is_last = idx == num_chunks - 1;

        // Convert i16 samples to normalized f32 (-1.0 to 1.0)
        for i in 0..OPUS_FRAME_SIZE {
            if i < chunk.len() {
                f32_frame[i] = (chunk[i] as f32) / 32768.0;
            } else {
                f32_frame[i] = 0.0; // Zero pad final frame if partial
            }
        }

        let encoded_len = encoder
            .encode(&f32_frame, OPUS_FRAME_SIZE, &mut packet_buf)
            .map_err(|e| anyhow!("Opus encode error at frame {idx}: {e:?}"))?;

        granule_pos += (OPUS_FRAME_SIZE as u64) * GRANULE_RATE_MULTIPLIER;

        let end_info = if is_last {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::NormalPacket
        };

        ogg_writer
            .write_packet(
                packet_buf[..encoded_len].to_vec().into_boxed_slice(),
                serial_number,
                end_info,
                granule_pos,
            )
            .map_err(|e| anyhow!("Failed to write Ogg packet at frame {idx}: {e:?}"))?;
    }

    Ok(out_buf.into_inner())
}

/// Constructs the 19-byte RFC 7845 `OpusHead` identification header.
fn build_opus_head(channels: u8, input_sample_rate: u32) -> Vec<u8> {
    let mut head = Vec::with_capacity(19);
    // Magic signature: 'OpusHead' (8 bytes)
    head.extend_from_slice(b"OpusHead");
    // Version: 1 (1 byte)
    head.push(1);
    // Channel count: 1 for Mono (1 byte)
    head.push(channels);
    // Pre-skip: 0 (2 bytes, little-endian)
    head.extend_from_slice(&0u16.to_le_bytes());
    // Original sample rate (4 bytes, little-endian)
    head.extend_from_slice(&input_sample_rate.to_le_bytes());
    // Output gain: 0 dB (2 bytes, little-endian)
    head.extend_from_slice(&0i16.to_le_bytes());
    // Channel mapping family: 0 (Mono or Stereo RTP layout)
    head.push(0);
    head
}

/// Constructs the RFC 7845 `OpusTags` comment header.
fn build_opus_tags(vendor: &str) -> Vec<u8> {
    let vendor_bytes = vendor.as_bytes();
    let mut tags = Vec::with_capacity(16 + vendor_bytes.len());
    // Magic signature: 'OpusTags' (8 bytes)
    tags.extend_from_slice(b"OpusTags");
    // Vendor string length (4 bytes, little-endian)
    tags.extend_from_slice(&(vendor_bytes.len() as u32).to_le_bytes());
    // Vendor string
    tags.extend_from_slice(vendor_bytes);
    // User comment list length: 0 comments (4 bytes, little-endian)
    tags.extend_from_slice(&0u32.to_le_bytes());
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_pcm_to_ogg_opus_header_and_structure() {
        // Generate 1 second of 440Hz sine wave at 16 kHz
        let mut samples = Vec::with_capacity(16000);
        for i in 0..16000 {
            let t = (i as f32) / 16000.0;
            let val = ((2.0 * std::f32::consts::PI * 440.0 * t).sin() * 16384.0) as i16;
            samples.push(val);
        }

        let ogg_bytes = encode_pcm_to_ogg_opus(&samples).expect("encoding failed");
        assert!(ogg_bytes.len() > 100, "encoded buffer too short");

        // First 4 bytes must be OggS magic
        assert_eq!(&ogg_bytes[..4], b"OggS");

        // Verify OpusHead signature exists within the first page
        let has_opus_head = ogg_bytes.windows(8).any(|w| w == b"OpusHead");
        assert!(has_opus_head, "OpusHead magic missing from stream");

        // Verify OpusTags signature exists
        let has_opus_tags = ogg_bytes.windows(8).any(|w| w == b"OpusTags");
        assert!(has_opus_tags, "OpusTags magic missing from stream");

        println!(
            "PCM 16kHz 1s (32,000 bytes) -> Ogg Opus: {} bytes (compression: {:.1}%)",
            ogg_bytes.len(),
            (1.0 - (ogg_bytes.len() as f32 / (samples.len() * 2) as f32)) * 100.0
        );
    }
}
