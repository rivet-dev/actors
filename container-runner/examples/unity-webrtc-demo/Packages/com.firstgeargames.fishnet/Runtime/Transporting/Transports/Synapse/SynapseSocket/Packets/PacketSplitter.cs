using System;
using System.Buffers;
using System.Collections.Generic;
using System.Threading;
using CodeBoost.Performance;

namespace SynapseSocket.Packets
{

    /// <summary>
    /// Send-side segmentation helper.
    /// Splits outbound payloads into wire-ready segment packets packed into a single rented backing buffer.
    /// Instances are managed by <see cref="ResettableObjectPool{T}"/>; rent via the pool, call <see cref="PacketSegmenter.Initialize"/> before use, and return via the pool when done.
    /// </summary>
    public sealed class PacketSplitter : PacketSegmenter
    {
        /// <summary>
        /// Monotonically increasing counter used to assign unique segment IDs to each split operation.
        /// </summary>
        private int _segmentIdCounter;

        /// <summary>
        /// Splits a payload into one or more wire-ready segments packed into a single rented backing buffer.
        /// Every element in the returned array is a slice of the same backing buffer — only <c>segments[0].Array</c> needs to be returned to <see cref="ArrayPool{T}.Shared"/>.
        /// The outer <see cref="ArraySegment{T}"/> array is rented from <see cref="ArrayPool{T}"/> of <see cref="ArraySegment{T}"/> and must also be returned separately.
        /// Use <paramref name="segmentCount"/> (not the array length) to iterate — the rented outer array may be larger than needed.
        /// </summary>
        /// <param name="payload">The application payload to split into segments.</param>
        /// <param name="isReliable">Whether the segments should be flagged for reliable delivery.</param>
        /// <param name="segmentCount">Receives the number of valid segments produced.</param>
        /// <param name="sequence">Reliable sequence number to embed in each segment header; used only when <paramref name="isReliable"/> is true.</param>
        /// <param name = "backingBuffer">All packets built into a single array.</param>
        /// <returns>A rented array of <see cref="ArraySegment{T}"/> values, each representing one wire-ready segment packet.</returns>
        public List<ArraySegment<byte>> Split(ReadOnlySpan<byte> payload, bool isReliable, out int segmentCount, ushort sequence, out byte[] backingBuffer)
        {
            PacketType type = isReliable ? PacketType.ReliableSegmented : PacketType.Segmented;
            int segmentPayloadSize = (int)MaximumTransmissionUnit - PacketHeader.ComputeHeaderSize(type);

            if (segmentPayloadSize <= 0)
                throw new InvalidOperationException("MTU too small for segmentation headers.");

            int totalSegments = (payload.Length + segmentPayloadSize - 1) / segmentPayloadSize;

            if (totalSegments > (int)MaximumSegments)
                throw new InvalidOperationException("Payload requires " + totalSegments + " segments but limit is " + MaximumSegments + ".");

            segmentCount = totalSegments;
            ushort segmentId = (ushort)Interlocked.Increment(ref _segmentIdCounter);
            int headerSize = PacketHeader.ComputeHeaderSize(type);

            // Single backing buffer: all N segment packets packed contiguously.
            // N * headerSize is a slight over-estimate because the last segment payload may be smaller, but renting a touch more is cheaper than computing the exact size.
            int totalBufferSize = totalSegments * headerSize + payload.Length;
            backingBuffer = ArrayPool<byte>.Shared.Rent(totalBufferSize);
            List<ArraySegment<byte>> segments = ListPool<ArraySegment<byte>>.Rent();

            int bufferOffset = 0;

            for (int i = 0; i < totalSegments; i++)
            {
                int segmentStartOffset = i * segmentPayloadSize;
                int segmentLength = Math.Min(segmentPayloadSize, payload.Length - segmentStartOffset);
                int written = PacketHeader.BuildPacket(backingBuffer.AsSpan(bufferOffset), type, sequence, segmentId, (byte)i, (byte)totalSegments, payload.Slice(segmentStartOffset, segmentLength));
                segments.Add(new(backingBuffer, bufferOffset, written));
                bufferOffset += written;
            }

            return segments;
        }

        /// <inheritdoc/>
        public override void OnReturn()
        {
            _segmentIdCounter = 0;
            base.OnReturn();
        }
    }
}
