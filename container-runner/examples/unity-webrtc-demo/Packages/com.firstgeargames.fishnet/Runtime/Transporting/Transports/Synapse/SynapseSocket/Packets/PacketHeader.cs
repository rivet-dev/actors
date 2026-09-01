using System;
using System.Runtime.CompilerServices;

namespace SynapseSocket.Packets
{

    /// <summary>
    /// Wire-format header helpers for Synapse packets.
    /// Layout:
    ///   [0]      : PacketType (1 byte)
    ///   [1..2]   : Sequence number (UInt16, little-endian) — only for <see cref="PacketType.Reliable"/>, <see cref="PacketType.Ack"/>, <see cref="PacketType.ReliableSegmented"/>
    ///   [3..4]   : Segment Id (UInt16) — only for <see cref="PacketType.Segmented"/> and <see cref="PacketType.ReliableSegmented"/>
    ///   [5]      : Segment Index (Byte) — only for <see cref="PacketType.Segmented"/> and <see cref="PacketType.ReliableSegmented"/>
    ///   [6]      : Segment Count (Byte) — only for <see cref="PacketType.Segmented"/> and <see cref="PacketType.ReliableSegmented"/>
    ///   [...]    : Payload
    /// Explicit little-endian ordering is used for cross-platform consistency.
    /// </summary>
    public static class PacketHeader
    {
        /// <summary>
        /// Size in bytes of the mandatory type field.
        /// </summary>
        public const int TypeSize = 1;

        /// <summary>
        /// Size in bytes of the reliable sequence field when present.
        /// </summary>
        public const int SequenceSize = 2;

        /// <summary>
        /// Size in bytes of the segmentation fields when present.
        /// </summary>
        public const int SegmentSize = 4;

        /// <summary>
        /// Maximum theoretical header size (all optional fields present).
        /// </summary>
        public const int MaxHeaderSize = TypeSize + SequenceSize + SegmentSize;

        /// <summary>
        /// Computes the header size for a given packet type.
        /// </summary>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static int ComputeHeaderSize(PacketType type) => type switch
        {
            PacketType.Reliable          => TypeSize + SequenceSize,
            PacketType.Ack               => TypeSize + SequenceSize,
            PacketType.Segmented         => TypeSize + SegmentSize,
            PacketType.ReliableSegmented => TypeSize + SequenceSize + SegmentSize,
            _                            => TypeSize
        };

        /// <summary>
        /// Writes a header into the supplied buffer starting at offset 0.
        /// Returns the number of bytes written.
        /// </summary>
        public static int Write(Span<byte> buffer, PacketType type, ushort sequence, ushort segmentId, byte segmentIndex, byte segmentCount)
        {
            int offset = 0;
            buffer[offset++] = (byte)type;

            if (type == PacketType.Reliable || type == PacketType.Ack || type == PacketType.ReliableSegmented)
            {
                buffer[offset++] = (byte)(sequence & 0xFF);
                buffer[offset++] = (byte)((sequence >> 8) & 0xFF);
            }

            if (type == PacketType.Segmented || type == PacketType.ReliableSegmented)
            {
                buffer[offset++] = (byte)(segmentId & 0xFF);
                buffer[offset++] = (byte)((segmentId >> 8) & 0xFF);
                buffer[offset++] = segmentIndex;
                buffer[offset++] = segmentCount;
            }

            return offset;
        }

        /// <summary>
        /// Writes a header followed by <paramref name="payload"/> into <paramref name="destination"/>.
        /// Returns the total number of bytes written (header + payload).
        /// </summary>
        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static int BuildPacket(Span<byte> destination, PacketType type, ushort sequence, ushort segmentId, byte segmentIndex, byte segmentCount, ReadOnlySpan<byte> payload)
        {
            int headerSize = Write(destination, type, sequence, segmentId, segmentIndex, segmentCount);
            payload.CopyTo(destination[headerSize..]);
            return headerSize + payload.Length;
        }

        /// <summary>
        /// Reads a header from the supplied buffer. Returns the number of bytes consumed.
        /// Throws if the buffer is too small for the declared type.
        /// </summary>
        public static int Read(ReadOnlySpan<byte> buffer, out PacketType type, out ushort sequence, out ushort segmentId, out byte segmentIndex, out byte segmentCount)
        {
            if (buffer.Length < TypeSize) throw new ArgumentException("Buffer too small for header.");

            int offset = 0;
            type = (PacketType)buffer[offset++];
            sequence = 0;
            segmentId = 0;
            segmentIndex = 0;
            segmentCount = 0;

            if (type == PacketType.Reliable || type == PacketType.Ack || type == PacketType.ReliableSegmented)
            {
                if (buffer.Length < offset + SequenceSize) throw new ArgumentException("Buffer too small for sequence.");
                sequence = (ushort)(buffer[offset] | (buffer[offset + 1] << 8));
                offset += 2;
            }

            if (type == PacketType.Segmented || type == PacketType.ReliableSegmented)
            {
                if (buffer.Length < offset + SegmentSize) throw new ArgumentException("Buffer too small for segment info.");
                segmentId = (ushort)(buffer[offset] | (buffer[offset + 1] << 8));
                segmentIndex = buffer[offset + 2];
                segmentCount = buffer[offset + 3];
                offset += 4;
            }

            return offset;
        }
    }
}
