using System;
using System.Buffers;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Net;
using System.Threading;
using CodeBoost.CodeAnalysis;
using CodeBoost.Extensions;
using CodeBoost.Performance;
using SynapseSocket.Core;
using SynapseSocket.Core.Configuration;
using SynapseSocket.Packets;
using SynapseSocket.Security;
using SynapseSocket.Transport;

namespace SynapseSocket.Connections
{

    /// <summary>
    /// Represents the state of a single remote peer session, including reliable send/receive windows, keep-alive timestamps, and signature binding.
    /// </summary>
    public sealed partial class SynapseConnection : IPoolResettable
    {
        /// <summary>
        /// Remote endpoint of this connection.
        /// </summary>
        [PoolResettableMember]
        public IPEndPoint RemoteEndPoint { get; private set; }
        /// <summary>
        /// The computed signature binding this connection to a physical identity.
        /// </summary>
        [PoolResettableMember]
        public ulong Signature { get; private set; }
        /// <summary>
        /// Index of this connection within <see cref="ConnectionManager._connections"/>.
        /// </summary>
        [PoolResettableMember]
        public int ConnectionsIndex { get; internal set; } = UnsetConnectionsIndex;
        /// <summary>
        /// Current lifecycle state.
        /// </summary>
        [PoolResettableMember]
        public ConnectionState State { get; internal set; }
        /// <summary>
        /// UTC ticks of the last received packet from this peer.
        /// </summary>
        [PoolResettableMember]
        public long LastReceivedTicks { get; internal set; }
        /// <summary>
        /// UTC ticks of the last sent keep-alive to this peer.
        /// </summary>
        [PoolResettableMember]
        public long LastKeepAliveSentTicks { get; internal set; }
        /// <summary>
        /// Number of consecutive keep-alives sent since the last received packet.
        /// Used to compute exponential backoff on the keep-alive send interval.
        /// Reset to zero whenever any inbound packet is received from this peer.
        /// </summary>
        [PoolResettableMember]
        internal int UnansweredKeepAlives;
        /// <summary>
        /// Next outbound reliable sequence number.
        /// </summary>
        [PoolResettableMember]
        internal ushort NextOutgoingSequence;
        /// <summary>
        /// Next expected inbound reliable sequence number (for ordered delivery).
        /// </summary>
        [PoolResettableMember]
        internal ushort NextExpectedSequence;
        /// <summary>
        /// Pending unacked reliable packets keyed by sequence.
        /// </summary>
        [PoolResettableMember]
        internal readonly ConcurrentDictionary<ushort, PendingReliable> PendingReliableQueue = new();
        /// <summary>
        /// Outbound ACK sequence numbers queued for batch delivery when ACK batching is enabled.
        /// </summary>
        [PoolResettableMember]
        internal readonly ConcurrentQueue<ushort> PendingAcks = new();
        /// <summary>
        /// Out-of-order reliable packets awaiting delivery.
        /// </summary>
        [PoolResettableMember]
        internal readonly Dictionary<ushort, ArraySegment<byte>> ReorderBuffer = new();
        /// <summary>
        /// Gate for reorder buffer and sequence manipulation.
        /// </summary>
        internal readonly object ReliableLock = new();
        /// <summary>
        /// Send-side splitter, rented from <see cref="CodeBoost.Performance.ResettableObjectPool{T}"/>
        /// on the first segmented send and returned to the pool on disconnect.
        /// Null until the first segmented send is issued on this connection.
        /// </summary>
        [PoolResettableMember]
        internal PacketSplitter? Splitter;
        /// <summary>
        /// Receive-side reassembler, rented from <see cref="CodeBoost.Performance.ResettableObjectPool{T}"/>
        /// on the first segmented receive and returned to the pool on disconnect.
        /// Null until the first segmented packet is received on this connection.
        /// </summary>
        [PoolResettableMember]
        internal PacketReassembler? Reassembler;
        /// <summary>
        /// Reference to the shared transmission engine used to send ACKs during batch flush.
        /// Set on connection establishment and cleared on disconnect.
        /// </summary>
        [PoolResettableMember]
        internal TransmissionEngine? TransmissionEngine;
        /// <summary>
        /// Value used when ConnectionsIndex is not set.
        /// </summary>
        public const int UnsetConnectionsIndex = -1;

        // ReSharper disable once EmptyConstructor
        public SynapseConnection() { }

        /// <summary>
        /// Creates a new connection record.
        /// </summary>
        /// <param name="remoteEndPoint">The peer's remote endpoint.</param>
        /// <param name="signature">The 64-bit signature that uniquely identifies this peer.</param>
        /// <param name = "connectionsIndex"></param>
        public void Initialize(IPEndPoint remoteEndPoint, ulong signature, int connectionsIndex)
        {
            RemoteEndPoint = remoteEndPoint ?? throw new ArgumentNullException(nameof(remoteEndPoint));
            Signature = signature;
            ConnectionsIndex = connectionsIndex;
            State = ConnectionState.Pending;
            LastReceivedTicks = DateTime.UtcNow.Ticks;
        }

        /// <summary>
        /// Dequeues all pending ACK sequence numbers and sends each via <see cref="TransmissionEngine"/>.
        /// Called by the maintenance loop when ACK batching is enabled.
        /// </summary>
        /// <param name="cancellationToken">Token forwarded to each ACK send.</param>
        internal void SendPendingAcks(CancellationToken cancellationToken)
        {
            while (PendingAcks.TryDequeue(out ushort sequence))
                _ = TransmissionEngine?.SendAckAsync(this, sequence, cancellationToken);
        }

        /// <summary>
        /// A reliable packet that has been sent but not yet acknowledged.
        /// Instances are managed by <see cref="ResettableObjectPool{T}"/>; rent via
        /// <see cref="ResettableObjectPool{T}.Rent"/> and return via <see cref="ReleasePendingReliable"/>
        /// so all pooled buffers and the <see cref="PendingReliable"/> object itself are recycled.
        /// <see cref="Segments"/> holds the wire-ready slices; <see cref="BackingArray"/> is the single
        /// rented buffer that backs all of them and is the only array returned to <see cref="ArrayPool{T}.Shared"/>.
        /// </summary>
        internal sealed class PendingReliable : IPoolResettable
        {
            /// <summary>
            /// Wire-ready slices of <see cref="BackingArray"/>, one per logical segment.
            /// For unsegmented sends this list contains exactly one entry.
            /// </summary>
            [PoolResettableMember]
            public List<ArraySegment<byte>> Segments { get; private set; }
            /// <summary>
            /// The single rented buffer that backs all entries in <see cref="Segments"/>.
            /// Returned to <see cref="ArrayPool{T}.Shared"/> on ACK or eviction.
            /// </summary>
            [PoolResettableMember]
            public byte[]? BackingArray { get; private set; }
            /// <summary>
            /// UTC ticks when this packet was last sent or retransmitted.
            /// </summary>
            [PoolResettableMember]
            public long SentTicks;
            /// <summary>
            /// Number of retransmission attempts so far.
            /// </summary>
            [PoolResettableMember]
            public int Retries;

            /// <summary>
            /// Initialises this instance for a reliable send.
            /// </summary>
            /// <param name="segments">Rented list of wire-ready slices of <paramref name="backingArray"/>.</param>
            /// <param name="backingArray">The single rented buffer backing all entries in <paramref name="segments"/>.</param>
            /// <param name="sentTicks">UTC ticks at the time of the initial send.</param>
            [PoolResettableMethod]
            public void Initialize(List<ArraySegment<byte>> segments, byte[] backingArray, long sentTicks)
            {
                Segments = segments;
                BackingArray = backingArray;
                SentTicks = sentTicks;
            }

            /// <inheritdoc/>
            public void OnRent() { }

            /// <inheritdoc/>
            public void OnReturn()
            {
                Retries = 0;
                SentTicks = 0;

                if (BackingArray is not null)
                {
                    ArrayPool<byte>.Shared.Return(BackingArray);
                    BackingArray = null;
                }

                if (Segments is not null)
                {
                    ListPool<ArraySegment<byte>>.Return(Segments);
                    Segments = null;
                }
            }
        }

        /// <summary>
        /// Returns all pooled memory held by <paramref name="pendingReliable"/> back to <see cref="ArrayPool{T}.Shared"/>
        /// and returns the <see cref="PendingReliable"/> instance itself to its <see cref="ResettableObjectPool{T}"/>.
        /// Safe to call from any context (ingress ACK path, maintenance sweep, or on kick).
        /// </summary>
        internal static void ReleasePendingReliable(PendingReliable pendingReliable)
        {
            ResettableObjectPool<PendingReliable>.Return(pendingReliable);
        }

        /// <summary>
        /// Drains the pending reliable queue of <paramref name="synapseConnection"/>, releasing every entry's
        /// pooled buffers and returning each <see cref="PendingReliable"/> to its pool.
        /// Call on connection teardown to avoid leaking rented buffers.
        /// </summary>
        internal static void DrainPendingReliableQueue(SynapseConnection synapseConnection)
        {
            foreach (KeyValuePair<ushort, PendingReliable> entry in synapseConnection.PendingReliableQueue)
                ReleasePendingReliable(entry.Value);

            synapseConnection.PendingReliableQueue.Clear();
        }

        /// <summary>
        /// Resets all per-session state for a reconnecting peer without returning the connection to the pool.
        /// Clears sequence numbers, the reorder buffer, pending ACKs, the pending reliable queue, and segmenters.
        /// Sets <see cref="State"/> to <see cref="ConnectionState.Disconnected"/> so the caller
        /// can re-initialise it through the normal handshake path.
        /// </summary>
        internal void ResetForReconnect()
        {
            PacketSplitter? splitter = Interlocked.Exchange(ref Splitter, null);
            if (splitter is not null)
                ResettableObjectPool<PacketSplitter>.Return(splitter);

            PacketReassembler? reassembler = Interlocked.Exchange(ref Reassembler, null);
            if (reassembler is not null)
                ResettableObjectPool<PacketReassembler>.Return(reassembler);

            DrainPendingReliableQueue(this);
            PendingAcks.Clear();

            lock (ReliableLock)
            {
                foreach (ArraySegment<byte> segment in ReorderBuffer.Values)
                    segment.PoolArrayIntoShared();

                ReorderBuffer.Clear();
                NextOutgoingSequence = 0;
                NextExpectedSequence = 0;
            }

            UnansweredKeepAlives = 0;
            LastKeepAliveSentTicks = 0;
            State = ConnectionState.Disconnected;
        }

        /// <inheritdoc/>
        public void OnReturn()
        {
            RemoteEndPoint = null;
            TransmissionEngine = null;
            Signature = SecurityProvider.UnsetSignature;
            ConnectionsIndex = UnsetConnectionsIndex;
            State = ConnectionState.Disconnected;

            LastReceivedTicks = 0;
            LastKeepAliveSentTicks = 0;
            UnansweredKeepAlives = 0;

            NextOutgoingSequence = 0;
            NextExpectedSequence = 0;
            PendingAcks.Clear();
            
            foreach (PendingReliable? value in PendingReliableQueue.Values)
                ResettableObjectPool<PendingReliable>.Return(value);

            PendingReliableQueue.Clear();

            foreach (ArraySegment<byte> reorderSegment in ReorderBuffer.Values)
                reorderSegment.PoolArrayIntoShared();

            ReorderBuffer.Clear();

            ResettableObjectPool<PacketSplitter>.ReturnAndNullifyReference(ref Splitter);
            ResettableObjectPool<PacketReassembler>.ReturnAndNullifyReference(ref Reassembler);
            
            /* Security. */
            _inboundRateCountersResetTick = 0;
            _receivedByPacketCount = 0;
            _receivedByBytesCount = 0;
        }

        /// <inheritdoc/>
        public void OnRent() { }
    }
}
