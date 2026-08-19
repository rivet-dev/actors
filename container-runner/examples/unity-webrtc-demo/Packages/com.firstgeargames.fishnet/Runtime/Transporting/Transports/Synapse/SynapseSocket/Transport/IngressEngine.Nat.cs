using System;
using System.Net;
using System.Security.Cryptography;
using System.Threading;
using SynapseSocket.Connections;
using SynapseSocket.Packets;
using SynapseSocket.Core.Configuration;

namespace SynapseSocket.Transport
{

    /// <summary>
    /// Ingress Engine (Receiver).
    /// Manages incoming data and initial filtering.
    /// Applies lowest-level mitigations BEFORE any payload copy.
    /// </summary>
    internal sealed partial class IngressEngine
    {
        /// <summary>
        /// True if NAT is enabled for any configuration.
        /// </summary>
        private bool _isNatEnabled;
        /// <summary>
        /// Size of the HMAC-SHA256 token truncated to this many bytes for NAT challenge packets.
        /// </summary>
        private const int NatTokenSize = 8;
        /// <summary>
        /// Duration of a single time bucket in ticks. Tokens are valid for the current bucket and the previous one (~60 seconds total).
        /// </summary>
        private const long NatTokenTimeBucketTicks = 30 * TimeSpan.TicksPerSecond;

        /// <summary>
        /// Handles an inbound NAT probe from an unrecognised endpoint.
        /// Responds with a challenge token instead of a handshake, subject to per-IP rate limiting.
        /// </summary>
        private void ProcessNatProbe(IPEndPoint fromEndPoint, CancellationToken cancellationToken)
        {
            if (!_isNatEnabled)
                return;

            // Never respond to blacklisted addresses.
            ulong signature = _security.ComputeSignature(fromEndPoint, ReadOnlySpan<byte>.Empty);

            if (_security.IsBlacklisted(signature))
                return;

            // Only respond to unrecognised endpoints; established peers do not need probes.
            if (_connections.ConnectionsByEndPoint.TryGetValue(fromEndPoint, out SynapseConnection? _))
                return;

            // Rate-limit outbound challenge responses per source IP.
            long nowTicks = DateTime.UtcNow.Ticks;
            long minIntervalTicks = _config.NatTraversal.IntervalMilliseconds * TimeSpan.TicksPerMillisecond;
            IpKey addressKey = IpKey.From(fromEndPoint.Address);

            long lastProbeEvict = Volatile.Read(ref _lastProbeEvictionTicks);

            if (nowTicks - lastProbeEvict > TimeSpan.TicksPerMinute)
            {
                if (Interlocked.CompareExchange(ref _lastProbeEvictionTicks, nowTicks, lastProbeEvict) == lastProbeEvict)
                    RemoveExpiredProbeLimitEntries(nowTicks, staleTicks: minIntervalTicks * 10);
            }

            long lastTicks = _natProbeLastResponseTicks.GetOrAdd(addressKey, 0L);

            if (nowTicks - lastTicks < minIntervalTicks)
                return;

            _natProbeLastResponseTicks[addressKey] = nowTicks;

            Span<byte> token = stackalloc byte[NatTokenSize];
            ComputeNatToken(fromEndPoint, nowTicks / NatTokenTimeBucketTicks, token);
            _ = _sender.SendNatChallengeAsync(fromEndPoint, token, cancellationToken);
        }

        /// <summary>
        /// Handles an inbound NatChallenge packet from an unrecognised endpoint.
        /// If the payload matches a token this engine issued, sends a handshake (completing the probe exchange).
        /// Otherwise echoes the token back — this is the initiator side of a simultaneous P2P probe.
        /// </summary>
        private void ProcessNatChallengeExchange(IPEndPoint fromEndPoint, ReadOnlySpan<byte> payload, CancellationToken cancellationToken)
        {
            if (!_isNatEnabled)
                return;

            if (payload.Length != NatTokenSize)
                return;

            ulong signature = _security.ComputeSignature(fromEndPoint, ReadOnlySpan<byte>.Empty);

            if (_security.IsBlacklisted(signature))
                return;

            if (_connections.ConnectionsByEndPoint.TryGetValue(fromEndPoint, out SynapseConnection? _))
                return;

            long nowTicks = DateTime.UtcNow.Ticks;
            long minIntervalTicks = _config.NatTraversal.IntervalMilliseconds * TimeSpan.TicksPerMillisecond;
            IpKey addressKey = IpKey.From(fromEndPoint.Address);

            long lastTicks = _natProbeLastResponseTicks.GetOrAdd(addressKey, 0L);

            if (nowTicks - lastTicks < minIntervalTicks)
                return;

            _natProbeLastResponseTicks[addressKey] = nowTicks;

            if (VerifyNatToken(fromEndPoint, payload))
                _ = _sender.SendHandshakeAsync(fromEndPoint, cancellationToken);
            else
                _ = _sender.SendNatChallengeAsync(fromEndPoint, payload, cancellationToken);
        }

        /// <summary>
        /// Computes a truncated HMAC-SHA256 token bound to <paramref name="endPoint"/> and <paramref name="timeBucket"/>.
        /// Writes exactly <see cref="NatTokenSize"/> bytes into <paramref name="destination"/>.
        /// </summary>
        private void ComputeNatToken(IPEndPoint endPoint, long timeBucket, Span<byte> destination)
        {
            Span<byte> addressBytes = stackalloc byte[16];
            endPoint.Address.TryWriteBytes(addressBytes, out int addressLength);

            int inputLength = addressLength + 2 + 8;
            Span<byte> input = stackalloc byte[inputLength];
            addressBytes[..addressLength].CopyTo(input);

            int offset = addressLength;
            input[offset++] = (byte)(endPoint.Port & 0xFF);
            input[offset++] = (byte)((endPoint.Port >> 8) & 0xFF);

            for (int i = 0; i < 8; i++)
                input[offset++] = (byte)((timeBucket >> (i * 8)) & 0xFF);

            Span<byte> hashBuffer = stackalloc byte[32];
            using HMACSHA256 hmac = new(_natChallengeSecret);
            hmac.TryComputeHash(input, hashBuffer, out _);
            hashBuffer[..NatTokenSize].CopyTo(destination);
        }

        /// <summary>
        /// Returns true if <paramref name="token"/> matches the expected token for the current or previous time bucket.
        /// </summary>
        private bool VerifyNatToken(IPEndPoint endPoint, ReadOnlySpan<byte> token)
        {
            long bucket = DateTime.UtcNow.Ticks / NatTokenTimeBucketTicks;
            Span<byte> expected = stackalloc byte[NatTokenSize];

            ComputeNatToken(endPoint, bucket, expected);
            if (token.SequenceEqual(expected))
                return true;

            ComputeNatToken(endPoint, bucket - 1, expected);
            return token.SequenceEqual(expected);
        }

        /// <summary>
        /// Evicts stale entries from the NAT probe response-time dictionary.
        /// </summary>
        private void RemoveExpiredProbeLimitEntries(long nowTicks, long staleTicks) => RemoveExpiredEntries(_natProbeLastResponseTicks, nowTicks, staleTicks);
    }
}
