using System.Collections.Generic;
using System.Net;

namespace SynapseSocket.Core.Configuration
{

    /// <summary>
    /// Configuration for a <see cref="SynapseManager"/> instance.
    /// All fields have sensible defaults; override only what you need.
    /// </summary>
    [System.Serializable]
    public sealed class SynapseConfig
    {
        // ReSharper disable FieldCanBeMadeReadOnly.Global

        /// <summary>
        /// Local endpoints to bind.
        /// At least one must be supplied before calling <see cref="SynapseManager.StartAsync"/>.
        /// </summary>
        [System.NonSerialized]
        public List<IPEndPoint> BindEndPoints = new();

        /// <summary>
        /// Maximum datagram size the engine will accept or send.
        /// Packets larger than this value are treated as oversized and trigger a violation.
        /// </summary>
        public uint MaximumPacketSize = 1400;

        /// <summary>
        /// Maximum transmission unit used for segmentation.
        /// Should be less than or equal to <see cref="MaximumPacketSize"/>.
        /// </summary>
        public uint MaximumTransmissionUnit = 1200;

        /// <summary>
        /// Maximum number of segments a segmented payload may be split into.
        /// Set to <see cref="DisabledMaximumSegments"/> to disable this feature.
        /// </summary>
        public uint MaximumSegments = DisabledMaximumSegments;

        /// <summary>
        /// Maximum number of concurrent incomplete segment assemblies per connection.
        /// Once reached, further segmented packets from that connection are treated as a protocol violation.
        /// Default is 16.
        /// </summary>
        public uint MaximumConcurrentSegmentAssembliesPerConnection = 16;

        /// <summary>
        /// Maximum number of simultaneous connections the engine will accept.
        /// Handshakes from new peers are rejected with <see cref="Core.Events.ConnectionRejectedReason.ServerFull"/>
        /// when the limit is reached. Set to 0 to disable.
        /// </summary>
        public uint MaximumConcurrentConnections = 0;


        /// <summary>
        /// Controls how the engine handles unreliable payloads that exceed the MTU.
        /// Defaults to <see cref="UnreliableSegmentMode.SegmentUnreliable"/>: oversized sends are split into unreliable segments.
        /// </summary>
        public UnreliableSegmentMode UnreliableSegmentMode = UnreliableSegmentMode.SegmentUnreliable;

        /// <summary>
        /// How long (in milliseconds) an incomplete segment assembly is kept before being evicted.
        /// Set to 0 to disable eviction.
        /// </summary>
        public uint SegmentAssemblyTimeoutMilliseconds = 5000;


        /// <summary>
        /// Kernel-level UDP socket receive buffer size (SO_RCVBUF) in bytes, applied on bind.
        /// Defaults to 1 MiB: the OS default (~64 KiB on Windows) is too small to absorb bursty
        /// fan-in from many peers sending segmented payloads concurrently, and undersized buffers
        /// cause silent datagram drops that also let a single noisy peer degrade delivery for
        /// other peers until the rate limiter kicks them. 1 MiB comfortably absorbs hundreds of
        /// concurrent segments and matches what most production realtime-UDP libraries default to.
        /// Set to <see cref="DisabledSocketBufferOverride"/> (0) to leave the OS default untouched.
        /// </summary>
        public int SocketReceiveBufferBytes = 1 * 1024 * 1024;

        /// <summary>
        /// Kernel-level UDP socket send buffer size (SO_SNDBUF) in bytes, applied on bind.
        /// Defaults to <see cref="DisabledSocketBufferOverride"/> (leave the OS default untouched) —
        /// send-side traffic is fan-out from a single process and paced by the app loop, so the
        /// typical OS default (~64 KiB) easily absorbs the small runs of segments a sender emits.
        /// Raise this only if you observe send-side back-pressure under genuine burst workloads.
        /// </summary>
        public int SocketSendBufferBytes = DisabledSocketBufferOverride;

        /// <summary>
        /// When true (default), received payloads are copied into a fresh buffer before being dispatched via <see cref="SynapseManager.PacketReceived"/>.
        /// The copy is recycled after the event returns; do not retain references to <see cref="SynapseSocket.Core.Events.PacketReceivedEventArgs.Payload"/> beyond the handler.
        /// When false, the internal payload buffer is dispatched directly and recycled immediately after the event returns.
        /// Copy payload data within the handler if it is needed beyond the callback.
        /// Note: reliable and segmented receives always copy internally regardless of this setting.
        /// </summary>
        public bool CopyReceivedPayloads = false;

        /// <summary>
        /// Enables telemetry counters.
        /// Has a minor performance cost; disable in production if not needed.
        /// </summary>
        public bool EnableTelemetry = false;

        /// <summary>
        /// Connection lifecycle settings: keep-alive interval, timeout, and sweep window.
        /// </summary>
        public ConnectionConfig Connection = new();

        /// <summary>
        /// Reliable delivery channel settings: pending queue limit, resend interval, and retry cap.
        /// </summary>
        public ReliableConfig Reliable = new();

        /// <summary>
        /// Latency simulation settings.
        /// Disabled by default.
        /// </summary>
        public LatencySimulatorConfig LatencySimulator = new();

        /// <summary>
        /// NAT traversal (hole punching) settings.
        /// Disabled by default; set <see cref="NatTraversalConfig.Mode"/> to enable.
        /// Has no effect when connecting to a server with a public IP (no NAT traversal required).
        /// </summary>
        public NatTraversalConfig NatTraversal = new();

        /// <summary>
        /// Security settings: rate limiting, replay protection, signature validation, and packet filtering.
        /// </summary>
        public SecurityConfig Security = new();

        /// <summary>
        /// Sentinel value: pass as <see cref="MaximumSegments"/> to disable segmentation.
        /// </summary>
        public const uint DisabledMaximumSegments = 0;

        /// <summary>
        /// Sentinel value: pass as <see cref="SegmentAssemblyTimeoutMilliseconds"/> to disable assembly timeout.
        /// </summary>
        public const uint DisabledSegmentAssemblyTimeout = 0;

        /// <summary>
        /// Sentinel value: pass as <see cref="SocketReceiveBufferBytes"/> or <see cref="SocketSendBufferBytes"/>
        /// to leave the OS default untouched.
        /// </summary>
        public const int DisabledSocketBufferOverride = 0;
    }
}
