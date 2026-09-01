using System.Threading;

namespace SynapseSocket.Diagnostics
{

    /// <summary>
    /// Optional high-performance counters.
    /// Uses <see cref="Interlocked"/> to avoid locks on hot paths.
    /// All counters are 64-bit.
    /// </summary>
    public sealed class Telemetry
    {
        /// <summary>
        /// True when telemetry is enabled.
        /// </summary>
        public bool IsEnabled { get; }

        /// <summary>
        /// Total bytes received.
        /// </summary>
        public long BytesIn => Interlocked.Read(ref _bytesIn);
        private long _bytesIn;

        /// <summary>
        /// Total bytes sent.
        /// </summary>
        public long BytesOut => Interlocked.Read(ref _bytesOut);
        private long _bytesOut;

        /// <summary>
        /// Total packets received.
        /// </summary>
        public long PacketsIn => Interlocked.Read(ref _packetsIn);
        private long _packetsIn;

        /// <summary>
        /// Total packets sent.
        /// </summary>
        public long PacketsOut => Interlocked.Read(ref _packetsOut);
        private long _packetsOut;

        /// <summary>
        /// Total incoming packets dropped by filtering.
        /// </summary>
        public long PacketsDroppedIn => Interlocked.Read(ref _packetsDroppedIn);
        private long _packetsDroppedIn;

        /// <summary>
        /// Total outgoing packets dropped (e.g., simulated loss).
        /// </summary>
        public long PacketsDroppedOut => Interlocked.Read(ref _packetsDroppedOut);
        private long _packetsDroppedOut;

        /// <summary>
        /// Total reliable retransmissions.
        /// </summary>
        public long ReliableResends => Interlocked.Read(ref _reliableResends);
        private long _reliableResends;

        /// <summary>
        /// Estimated lost packets (reliable retry exhaustion).
        /// </summary>
        public long PacketsLost => Interlocked.Read(ref _packetsLost);
        private long _packetsLost;

        /// <summary>
        /// Creates a telemetry instance.
        /// </summary>
        /// <param name="isEnabled">Whether telemetry collection is active.</param>
        public Telemetry(bool isEnabled)
        {
            IsEnabled = isEnabled;
        }

        /// <summary>
        /// Records an inbound packet, incrementing both the byte and packet counters.
        /// </summary>
        /// <param name="bytes">Number of bytes in the received packet.</param>
        internal void OnReceived(int bytes)
        {
            if (!IsEnabled) return;
            Interlocked.Add(ref _bytesIn, bytes);
            Interlocked.Increment(ref _packetsIn);
        }

        /// <summary>
        /// Records an outbound packet, incrementing both the byte and packet counters.
        /// </summary>
        /// <param name="bytes">Number of bytes in the sent packet.</param>
        internal void OnSent(int bytes)
        {
            if (!IsEnabled) return;
            Interlocked.Add(ref _bytesOut, bytes);
            Interlocked.Increment(ref _packetsOut);
        }

        /// <summary>
        /// Records an inbound packet that was dropped by the security filter.
        /// </summary>
        internal void OnSecurityDroppedReceived()
        {
            if (!IsEnabled) return;
            Interlocked.Increment(ref _packetsDroppedIn);
        }

        /// <summary>
        /// Records an outbound packet that was dropped by the latency simulator.
        /// </summary>
        internal void OnLatencyDroppedSent()
        {
            if (!IsEnabled) return;
            Interlocked.Increment(ref _packetsDroppedOut);
        }

        /// <summary>
        /// Records a reliable-channel retransmission triggered by the maintenance sweep.
        /// </summary>
        internal void OnReliableResend()
        {
            if (!IsEnabled) return;
            Interlocked.Increment(ref _reliableResends);
        }

        /// <summary>
        /// Records a packet considered lost after exhausting all reliable retransmission attempts.
        /// </summary>
        internal void OnLost()
        {
            if (!IsEnabled) return;
            Interlocked.Increment(ref _packetsLost);
        }
    }
}
