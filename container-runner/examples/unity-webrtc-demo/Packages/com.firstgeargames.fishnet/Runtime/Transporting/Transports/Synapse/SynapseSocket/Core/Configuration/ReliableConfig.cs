namespace SynapseSocket.Core.Configuration
{

    /// <summary>
    /// Configuration for the reliable delivery channel.
    /// </summary>
    [System.Serializable]
    public sealed class ReliableConfig
    {
        // ReSharper disable FieldCanBeMadeReadOnly.Global

        /// <summary>
        /// Maximum number of unacknowledged reliable packets per connection before backpressure is applied.
        /// </summary>
        public uint MaximumPending = 256;

        /// <summary>
        /// Time in milliseconds before an unacknowledged reliable packet is retransmitted.
        /// </summary>
        public uint ResendMilliseconds = 250;

        /// <summary>
        /// Maximum number of retransmission attempts before the connection is terminated.
        /// </summary>
        public uint MaximumRetries = 10;

        /// <summary>
        /// When true, outgoing ACKs are queued and flushed in batches rather than sent immediately per packet.
        /// Reduces ACK traffic under burst receive conditions at the cost of a small delivery delay.
        /// Flush interval is controlled by <see cref="AckBatchIntervalMilliseconds"/>.
        /// </summary>
        public bool AckBatchingEnabled = true;

        /// <summary>
        /// Milliseconds between ACK batch flushes when <see cref="AckBatchingEnabled"/> is true.
        /// Lower values reduce ACK latency; higher values improve batching efficiency under bursts.
        /// </summary>
        public uint AckBatchIntervalMilliseconds = 20;

        /// <summary>
        /// Minimum permitted value for <see cref="AckBatchIntervalMilliseconds"/>. Values below this are clamped up.
        /// </summary>
        public const uint MinimumAckBatchIntervalMilliseconds = 20;
        /// <summary>
        /// Maximum permitted value for <see cref="AckBatchIntervalMilliseconds"/>. Values above this are clamped down.
        /// </summary>
        public const uint MaximumAckBatchIntervalMilliseconds = 1000;

    }
}
