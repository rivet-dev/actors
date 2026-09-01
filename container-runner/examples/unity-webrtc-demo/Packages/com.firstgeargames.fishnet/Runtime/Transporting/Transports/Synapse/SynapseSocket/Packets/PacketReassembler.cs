using System;
using System.Buffers;
using System.Collections.Generic;
using CodeBoost.CodeAnalysis;
using CodeBoost.Extensions;
using CodeBoost.Performance;

namespace SynapseSocket.Packets
{

    /// <summary>
    /// Receive-side segmentation helper.
    /// Feeds arriving segment packets into per-stream reassembly buffers and emits the fully rebuilt payload when all segments have arrived.
    /// Instances are managed by <see cref="ResettableObjectPool{T}"/>; rent via the pool, call <see cref="PacketSegmenter.Initialize"/> before use, and return via the pool when done.
    /// </summary>
    public sealed class PacketReassembler : PacketSegmenter
    {
        /// <summary>
        /// Maximum number of concurrent assemblies permitted.
        /// <see cref="UnsetMaximumConcurrentAssemblies"/> disables the limit.
        /// </summary>
        private uint _maximumConcurrentAssemblies;
        /// <summary>
        /// Active in-progress assemblies keyed by segment ID.
        /// </summary>
        private readonly Dictionary<ushort, SegmentAssembly> _currentSegments = new();
        /// <summary>
        /// Guards access to <see cref="_currentSegments"/>.
        /// </summary>
        private readonly object _lock = new();
        /// <summary>
        /// Sentinel value for <see cref="_maximumConcurrentAssemblies"/> that disables the concurrent assembly limit.
        /// </summary>
        public const uint UnsetMaximumConcurrentAssemblies = 0;

        /// <summary>
        /// Configures the reassembler after renting from the pool.
        /// </summary>
        /// <param name="maximumTransmissionUnit">Maximum wire packet size in bytes.</param>
        /// <param name="maximumSegments">Maximum number of segments a single message may be split into.</param>
        /// <param name="maximumConcurrentAssemblies">Maximum number of simultaneous in-progress assemblies; <see cref="UnsetMaximumConcurrentAssemblies"/> disables the limit.</param>
        public void Initialize(uint maximumTransmissionUnit, uint maximumSegments, uint maximumConcurrentAssemblies)
        {
            base.Initialize(maximumTransmissionUnit, maximumSegments);
            _maximumConcurrentAssemblies = maximumConcurrentAssemblies;
        }

        /// <summary>
        /// Feeds a received segment into the reassembly buffer.
        /// Returns true and outputs the fully reassembled payload when the final segment arrives.
        /// The caller is responsible for returning <paramref name="assembledSegments"/>.Array to <see cref="ArrayPool{T}.Shared"/> when done.
        /// The completed <see cref="SegmentAssembly"/> is automatically returned to the pool.
        /// If a protocol violation is detected (e.g. the same segmentId is seen with a different segmentCount or reliability flag than previously declared), <paramref name="isProtocolViolation"/> is set to true, the stale assembly is evicted, and the method returns false.
        /// Callers should treat this as grounds for kicking/blacklisting the peer.
        /// </summary>
        /// <param name="segmentId">Stream identifier shared by all segments of the same logical message.</param>
        /// <param name="segmentIndex">Zero-based position of this segment within the stream.</param>
        /// <param name="segmentCount">Total number of segments in the stream.</param>
        /// <param name="segmentData">Raw payload bytes for this segment.</param>
        /// <param name="isReliable">Whether the segment was delivered on the reliable channel.</param>
        /// <param name="assembledSegments">Receives the fully reassembled payload when the method returns true.</param>
        /// <param name="isProtocolViolation">Set to true when a protocol inconsistency is detected; false otherwise.</param>
        /// <returns>True when the final segment has arrived and the payload is fully reassembled; otherwise false.</returns>
        public bool TryReassemble(ushort segmentId, byte segmentIndex, byte segmentCount, ReadOnlySpan<byte> segmentData, bool isReliable, out ArraySegment<byte> assembledSegments, out bool isProtocolViolation)
        {
            assembledSegments = default;
            isProtocolViolation = false;

            if (segmentCount == 0 || segmentCount > MaximumSegments || segmentIndex >= segmentCount)
                return false;

            lock (_lock)
            {
                if (!_currentSegments.TryGetValue(segmentId, out SegmentAssembly? segmentAssembly))
                {
                    if (_maximumConcurrentAssemblies > 0 && _currentSegments.Count >= _maximumConcurrentAssemblies)
                    {
                        isProtocolViolation = true;
                        return false;
                    }

                    segmentAssembly = ResettableObjectPool<SegmentAssembly>.Rent();
                    segmentAssembly.Initialize(segmentCount, isReliable);
                    _currentSegments[segmentId] = segmentAssembly;
                }
                else if (segmentAssembly.SegmentCount != segmentCount || segmentAssembly.IsReliable != isReliable)
                {
                    // Same segmentId reused with a different declared segmentCount or reliability flag.
                    // This is either a sender bug or a malicious attempt to desync reassembly state.
                    // Evict the stale assembly and signal a protocol violation to the caller.
                    _currentSegments.Remove(segmentId);
                    ResettableObjectPool<SegmentAssembly>.Return(segmentAssembly);
                    isProtocolViolation = true;

                    return false;
                }

                segmentAssembly.Add(segmentIndex, segmentData);

                if (segmentAssembly.TryGetAssembledSegments(out assembledSegments))
                {
                    _currentSegments.Remove(segmentId);
                    ResettableObjectPool<SegmentAssembly>.Return(segmentAssembly);

                    return true;
                }
            }

            return false;
        }

        /// <summary>
        /// Removes and returns to pool incomplete segment assemblies that have exceeded the timeout.
        /// This ensures that assemblies from connections that disconnect before completing are not held indefinitely.
        /// </summary>
        /// <param name="nowTicks">Current timestamp in <see cref="System.DateTime.Ticks"/>.</param>
        /// <param name="timeoutTicks">Maximum age in ticks before an incomplete assembly is evicted.</param>
        public void RemoveExpiredSegments(long nowTicks, long timeoutTicks)
        {
            lock (_lock)
            {
                List<ushort> toRemove = ListPool<ushort>.Rent();

                try
                {
                    foreach (KeyValuePair<ushort, SegmentAssembly> keyValuePair in _currentSegments)
                    {
                        if (nowTicks - keyValuePair.Value.FirstReceivedTicks > timeoutTicks)
                            toRemove.Add(keyValuePair.Key);
                    }

                    foreach (ushort id in toRemove)
                    {
                        if (_currentSegments.Remove(id, out SegmentAssembly? evicted))
                            ResettableObjectPool<SegmentAssembly>.Return(evicted);
                    }
                }
                finally
                {
                    ListPool<ushort>.Return(toRemove);
                }
            }
        }

        /// <inheritdoc/>
        public override void OnReturn()
        {
            lock (_lock)
            {
                foreach (SegmentAssembly segmentAssembly in _currentSegments.Values)
                    ResettableObjectPool<SegmentAssembly>.Return(segmentAssembly);

                _currentSegments.Clear();
            }

            _maximumConcurrentAssemblies = UnsetMaximumConcurrentAssemblies;
            base.OnReturn();
        }

        /// <summary>
        /// Tracks the in-progress reassembly of a single segmented payload identified by its segment ID.
        /// Stores arriving segments by index and produces the final payload once all have arrived.
        /// </summary>
        private sealed class SegmentAssembly : IPoolResettable
        {
            /// <summary>
            /// Expected total number of segments for this payload.
            /// </summary>
            [PoolResettableMember]
            public uint SegmentCount { get; private set; }
            /// <summary>
            /// Tick timestamp of the first segment received, used for assembly timeout eviction.
            /// </summary>
            [PoolResettableMember]
            public long FirstReceivedTicks;
            /// <summary>
            /// Whether this assembly belongs to the reliable channel.
            /// </summary>
            [PoolResettableMember]
            public bool IsReliable;

            // _segments is rented from ListPool on Initialize; each slot is pre-filled with null so segments can be stored by index as they arrive (out-of-order safe).
            // _lengths tracks each segment's actual data length (ArrayPool may over-allocate).
            [PoolResettableMember]
            private List<ArraySegment<byte>>? _segments;
            /// <summary>
            /// Count of received segments.
            /// </summary>
            [PoolResettableMember]
            private uint _receivedCount;
            /// <summary>
            /// Total length of added segments.
            /// </summary>
            [PoolResettableMember]
            private uint _totalLength;

            /// <summary>
            /// Initializes a new <see cref="SegmentAssembly"/> instance.
            /// </summary>
            public SegmentAssembly() { }

            /// <summary>
            /// Prepares the assembly for a new segmented payload with the given segment count and reliability flag.
            /// </summary>
            /// <param name="segmentCount">Total number of segments expected for this payload.</param>
            /// <param name="isReliable">Whether the segments belong to the reliable channel.</param>
            [PoolResettableMethod]
            public void Initialize(byte segmentCount, bool isReliable)
            {
                SegmentCount = segmentCount;
                IsReliable = isReliable;

                _segments = ListPool<ArraySegment<byte>>.Rent();
                for (int i = _segments.Count; i < segmentCount; i++)
                    _segments.Add(null);
            }

            /// <summary>
            /// Stores a segment at the given index, copying its data into a pooled buffer.
            /// Duplicate indices are silently ignored.
            /// </summary>
            /// <param name="segmentIndex">Zero-based index of the segment to store.</param>
            /// <param name="segmentData">Raw bytes for this segment.</param>
            public void Add(byte segmentIndex, ReadOnlySpan<byte> segmentData)
            {
                if (_segments![segmentIndex].Array is not null)
                    return;

                if (_receivedCount == 0)
                    FirstReceivedTicks = DateTime.UtcNow.Ticks;

                byte[] rentedBuffer = ArrayPool<byte>.Shared.Rent(segmentData.Length);
                segmentData.CopyTo(rentedBuffer);

                int segmentLength = segmentData.Length;
                _segments[segmentIndex] = new(rentedBuffer, offset: 0, segmentLength);

                _receivedCount++;
                _totalLength += (uint)segmentLength;
            }

            /// <summary>
            /// Copies all segments into a single rented buffer and returns it as an <see cref="ArraySegment{T}"/> scoped to the exact reassembled length.
            /// The caller is responsible for returning <see cref="ArraySegment{T}.Array"/> to <see cref="ArrayPool{T}.Shared"/> when done.
            /// </summary>
            /// <param name="assembledSegment">Receives the fully assembled payload when the method returns true.</param>
            /// <returns>True when all segments have been received and the payload has been assembled; otherwise false.</returns>
            public bool TryGetAssembledSegments(out ArraySegment<byte> assembledSegment)
            {
                // Exit if all segments have not been received.
                if (_receivedCount != SegmentCount)
                {
                    assembledSegment = default;
                    return false;
                }

                byte[] reassembledBytes = ArrayPool<byte>.Shared.Rent((int)_totalLength);

                int offset = 0;
                for (int i = 0; i < SegmentCount; i++)
                {
                    int segmentLength = _segments![i].Count;
                    Buffer.BlockCopy(_segments![i].Array!, srcOffset: 0, dst: reassembledBytes, offset, count: segmentLength);

                    offset += segmentLength;
                }

                assembledSegment = new(reassembledBytes, offset: 0, (int)_totalLength);
                return true;
            }

            /// <inheritdoc/>
            public void OnRent() { }

            /// <inheritdoc/>
            public void OnReturn()
            {
                if (_segments is not null)
                {
                    foreach (ArraySegment<byte> arraySegment in _segments)
                        arraySegment.PoolArrayIntoShared();

                    ListPool<ArraySegment<byte>>.ReturnAndNullifyReference(ref _segments);
                }

                SegmentCount = 0;
                _receivedCount = 0;
                _totalLength = 0;
                FirstReceivedTicks = 0;
            }
        }
    }
}
