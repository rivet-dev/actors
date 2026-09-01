using FishNet.Transporting.Synapse;
using SynapseSocket.Security;

namespace SynapseSocket.Core.Configuration
{

    /// <summary>
    /// Security settings for a <see cref="SynapseSocket.Core.SynapseManager"/> instance.
    /// Controls rate limiting, replay protection, signature validation, and packet filtering.
    /// </summary>
    [System.Serializable]
    public sealed class SecurityConfig
    {
        // ReSharper disable FieldCanBeMadeReadOnly.Global

        /// <summary>
        /// Maximum packets per second allowed per signature.
        /// Set to <see cref="DisabledMaximumPacketsPerSecond"/> (0) to disable packet rate limiting.
        /// </summary>
        public uint MaximumPacketsPerSecond = 500;

        /// <summary>
        /// Maximum received bytes per second allowed per signature.
        /// Paired with <see cref="MaximumPacketsPerSecond"/>: the packet count alone cannot catch
        /// a peer sending near the pps cap at maximum packet size, which would sustain
        /// <c>MaximumPacketsPerSecond * MaximumPacketSize</c> bytes/sec — well above a realistic
        /// realtime-game upstream. Defaults to 2 MiB/s, which allows comfortable legitimate headroom
        /// while cutting off bandwidth floods.
        /// Set to <see cref="DisabledMaximumBytesPerSecond"/> (0) to disable bytes rate limiting.
        /// </summary>
        public uint MaximumBytesPerSecond = 2 * 1024 * 1024;

        /// <summary>
        /// Maximum number of out-of-order reliable packets buffered per connection before raising a violation.
        /// Default is 64. Set to 0 to disable.
        /// </summary>
        public uint MaximumOutOfOrderReliablePackets = 64;

        /// <summary>
        /// Maximum reassembled payload size in bytes.
        /// If a segment header declares a segment count such that <c>segmentCount * MaximumTransmissionUnit</c>
        /// exceeds this value, the sender is immediately blacklisted.
        /// Set to 0 to disable this check.
        /// </summary>
        public uint MaximumReassembledPacketSize = 0;

        /// <summary>
        /// Optional custom signature provider asset.
        /// Assign a <see cref="SignatureProviderBase"/> ScriptableObject in the inspector to override the default FNV-1a endpoint hash.
        /// Applied at transport start; takes precedence over <see cref="SignatureProvider"/> when non-null.
        /// </summary>
        public SignatureProviderBase? SignatureProviderAsset;

        /// <summary>
        /// Optional signature validator asset applied during handshake.
        /// Assign a <see cref="SignatureValidatorBase"/> ScriptableObject in the inspector to enforce allow-lists or application-specific identity checks.
        /// Applied at transport start; takes precedence over <see cref="SignatureValidator"/> when non-null.
        /// </summary>
        public SignatureValidatorBase? SignatureValidatorAsset;

        /// <summary>
        /// Optional custom signature provider set at runtime.
        /// Defaults to <see cref="DefaultSignatureProvider"/> when null and <see cref="SignatureProviderAsset"/> is also null.
        /// </summary>
        [System.NonSerialized]
        public ISignatureProvider? SignatureProvider;

        /// <summary>
        /// Optional signature validator set at runtime, applied during handshake.
        /// When null and <see cref="SignatureValidatorAsset"/> is also null, all valid signatures are accepted.
        /// </summary>
        [System.NonSerialized]
        public ISignatureValidator? SignatureValidator;

        /// <summary>
        /// When true, datagrams whose first byte does not match any known <see cref="SynapseSocket.Packets.PacketType"/>
        /// are passed to the <see cref="SynapseSocket.Core.SynapseManager.UnknownPacketReceived"/> delegate, which returns a
        /// <see cref="SynapseSocket.Security.FilterResult"/> to indicate whether the packet is accepted.
        /// A result other than <see cref="SynapseSocket.Security.FilterResult.Allowed"/> raises a violation.
        /// When false (default), any such datagram immediately raises a violation without invoking the delegate.
        /// Enable this only when an external protocol (e.g. a rendezvous/beacon client) intentionally
        /// piggybacks on the Synapse UDP socket.
        /// </summary>
        public bool AllowUnknownPackets = false;

        /// <summary>
        /// When true, the handshake replay cache is bypassed and duplicate handshake packets are accepted.
        /// Intended for testing reconnect scenarios only — never set this in production.
        /// </summary>
        public bool DisableHandshakeReplayProtection = false;

        /// <summary>
        /// Sentinel value: pass as <see cref="MaximumPacketsPerSecond"/> to disable packet rate limiting.
        /// </summary>
        public const uint DisabledMaximumPacketsPerSecond = 0;

        /// <summary>
        /// Sentinel value: pass as <see cref="MaximumBytesPerSecond"/> to disable bytes rate limiting.
        /// </summary>
        public const uint DisabledMaximumBytesPerSecond = 0;
    }
}
