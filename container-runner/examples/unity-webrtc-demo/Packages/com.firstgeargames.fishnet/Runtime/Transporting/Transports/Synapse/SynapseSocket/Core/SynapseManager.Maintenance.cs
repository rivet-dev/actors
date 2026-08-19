using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using SynapseSocket.Connections;
using SynapseSocket.Core.Configuration;
using SynapseSocket.Core.Events;

namespace SynapseSocket.Core
{

    /// <summary>
    /// Background maintenance for <see cref="SynapseManager"/>: progressive keep-alive sweeps, timeout detection, and reliable retransmission.
    /// Implemented as a partial class to separate feature sets per the spec.
    /// </summary>
    public sealed partial class SynapseManager
    {
        private const string ViolationReliableExhausted = "Connection exceeded the maximum reliable packet retry limit.";
        /// <summary>
        /// Ticks between keep-alive heartbeats, derived from <see cref="SynapseSocket.Core.Configuration.ConnectionConfig.KeepAliveIntervalMilliseconds"/>.
        /// </summary>
        private readonly long _connectionKeepAliveTicks;
        /// <summary>
        /// Ticks of idle time after which a connection is considered timed out, derived from <see cref="SynapseSocket.Core.Configuration.ConnectionConfig.TimeoutMilliseconds"/>.
        /// </summary>
        private readonly long _connectionTimeoutTicks;
        /// <summary>
        /// Ticks between reliable packet retransmission attempts, derived from <see cref="SynapseSocket.Core.Configuration.ReliableConfig.ResendMilliseconds"/>.
        /// </summary>
        private readonly long _reliableResendTicks;
        /// <summary>
        /// Maximum number of retransmission attempts before a reliable packet is considered lost, derived from <see cref="SynapseSocket.Core.Configuration.ReliableConfig.MaximumRetries"/>.
        /// </summary>
        private readonly uint _maximumReliableRetries;
        /// <summary>
        /// Ticks after which an incomplete segment assembly is evicted. Set to <see cref="UnsetSegmentAssemblyTimeoutTicks"/> when the timeout is disabled or segmentation is off.
        /// </summary>
        private readonly long _segmentAssemblyTimeoutTicks;
        /// <summary>
        /// Maximum number of packets a single connection may receive per second before further packets are dropped.
        /// Derived from <see cref="SynapseSocket.Core.Configuration.SecurityConfig.MaximumPacketsPerSecond"/>.
        /// Set to <see cref="SynapseSocket.Core.Configuration.SecurityConfig.DisabledMaximumPacketsPerSecond"/> when the limit is disabled.
        /// </summary>
        private readonly uint _maximumPacketsPerSecond;
        /// <summary>
        /// Maximum number of bytes a single connection may receive per second before further packets are dropped.
        /// Derived from <see cref="SynapseSocket.Core.Configuration.SecurityConfig.MaximumBytesPerSecond"/>.
        /// Set to <see cref="SynapseSocket.Core.Configuration.SecurityConfig.DisabledMaximumBytesPerSecond"/> when the limit is disabled.
        /// </summary>
        private readonly uint _maximumBytesPerSecond;
        /// <summary>
        /// Cached value of <see cref="SynapseSocket.Core.Configuration.ReliableConfig.AckBatchingEnabled"/> to avoid repeated config lookups on the hot maintenance path.
        /// </summary>
        private readonly bool _isAckBatchingEnabled;
        /// <summary>
        /// Index of the connection to process on the next maintenance tick. Wraps around when it reaches the connection count.
        /// </summary>
        private int _nextMaintenanceConnectionIndex;
        /// <summary>
        /// Index of the connection to flush pending ACKs for on the next ACK loop tick. Wraps around when it reaches the connection count.
        /// </summary>
        private int _nextPendingAckConnectionIndex;
        /// <summary>
        /// Sentinel value indicating that ACK batching interval is unset (batching disabled).
        /// </summary>
        public const long UnsetAckBatchingIntervalTicks = 0;
        /// <summary>
        /// Sentinel value indicating that segment assembly timeout is disabled.
        /// </summary>
        private const long UnsetSegmentAssemblyTimeoutTicks = 0;

        /// <summary>
        /// Background loop that periodically runs keep-alive, retransmit, and segment-timeout sweeps.
        /// </summary>
        private async Task MaintenanceLoopAsync(CancellationToken cancellationToken)
        {
            const int MaintenanceLoopTargetMilliseconds = 50;

            while (!cancellationToken.IsCancellationRequested)
            {
                if (!DoConnectionsExist())
                {
                    await Task.Delay(MaintenanceLoopTargetMilliseconds, cancellationToken).ConfigureAwait(false);
                    continue;
                }

                int connectionsCount = Connections.Connections.Count;
                int connectionsPerTick = Math.Max(1, connectionsCount / MaintenanceLoopTargetMilliseconds);

                try
                {
                    long nowTicks = DateTime.UtcNow.Ticks;

                    for (int i = 0; i < connectionsPerTick; i++)
                    {
                        SynapseConnection connection = GetConnectionAndIncreaseIndex(connectionsCount, ref _nextMaintenanceConnectionIndex);

                        if (PerformKeepAlive(nowTicks, connection, cancellationToken))
                        {
                            RetransmitReliable(nowTicks, connection, cancellationToken);
                            TimeoutAssembledSegments(nowTicks, connection);
                            ResetInboundRateCounters(nowTicks, connection);
                        }
                    }
                }
                catch (OperationCanceledException)
                {
                    break;
                }
                catch (Exception unexpectedException)
                {
                    UnhandledException?.Invoke(unexpectedException);
                }

                try
                {
                    await WaitDelayForLoop(MaintenanceLoopTargetMilliseconds, connectionsCount, cancellationToken);
                }
                catch (OperationCanceledException)
                {
                    break;
                }
            }
        }

        /// <summary>
        /// Background loop that flushes batched outbound ACKs for each connection at the configured <see cref="Configuration.ReliableConfig.AckBatchIntervalMilliseconds"/> interval.
        /// Only started when ACK batching is enabled.
        /// </summary>
        private async Task PendingAckLoopAsync(CancellationToken cancellationToken)
        {
            /* Enabled state of batching does not need to be checked.
             * This Task will not start if batching is disabled. */

            int pendingAckLoopTargetMilliseconds = (int)Math.Clamp(Config.Reliable.AckBatchIntervalMilliseconds, ReliableConfig.MinimumAckBatchIntervalMilliseconds, ReliableConfig.MaximumAckBatchIntervalMilliseconds);

            while (!cancellationToken.IsCancellationRequested)
            {
                if (!DoConnectionsExist())
                {
                    await Task.Delay(pendingAckLoopTargetMilliseconds, cancellationToken).ConfigureAwait(false);
                    continue;
                }

                int connectionsCount = Connections.Connections.Count;
                int connectionsPerTick = Math.Max(1, connectionsCount / pendingAckLoopTargetMilliseconds);

                try
                {
                    for (int i = 0; i < connectionsPerTick; i++)
                    {
                        SynapseConnection connection = GetConnectionAndIncreaseIndex(connectionsCount, ref _nextPendingAckConnectionIndex);
                        connection.SendPendingAcks(cancellationToken);
                    }
                }
                catch (OperationCanceledException)
                {
                    break;
                }
                catch (Exception unexpectedException)
                {
                    UnhandledException?.Invoke(unexpectedException);
                }

                try
                {
                    await WaitDelayForLoop(pendingAckLoopTargetMilliseconds, connectionsCount, cancellationToken);
                }
                catch (OperationCanceledException)
                {
                    break;
                }
            }
        }

        /// <summary>
        /// Returns the connection at <paramref name="currentIndex"/>, wrapping the index to zero when it reaches <paramref name="connectionsCount"/>, then advances it.
        /// </summary>
        /// <param name="connectionsCount">Current snapshot of the connection count, used for wrap-around.</param>
        /// <param name="currentIndex">The index to read and advance. Passed by ref so the caller's field is updated.</param>
        private SynapseConnection GetConnectionAndIncreaseIndex(int connectionsCount, ref int currentIndex)
        {
            if (currentIndex >= connectionsCount)
                currentIndex = 0;

            SynapseConnection connection = Connections.Connections[currentIndex];
            currentIndex++;
            return connection;
        }

        /// <summary>
        /// Returns true when the connection list is non-null and has at least one entry.
        /// </summary>
        private bool DoConnectionsExist() => Connections is not null && Connections.Connections.Count > 0;

        /// <summary>
        /// Delays for a per-connection slice of <paramref name="targetMilliseconds"/> so all connections are visited within approximately that window.
        /// </summary>
        /// <param name="targetMilliseconds">The total sweep window in milliseconds.</param>
        /// <param name="connectionsCount">Current connection count used to calculate the per-connection wait slice.</param>
        /// <param name="cancellationToken">Token forwarded to <see cref="Task.Delay(int, CancellationToken)"/>.</param>
        private async Task WaitDelayForLoop(int targetMilliseconds, int connectionsCount, CancellationToken cancellationToken)
        {
            /* Wait time is calculated to have iterated all connections
             * at roughly the target milliseconds. */
            int waitMilliseconds = Math.Max(1, targetMilliseconds / connectionsCount);
            await Task.Delay(waitMilliseconds, cancellationToken).ConfigureAwait(false);
        }

        /// <summary>
        /// Progressive keep-alive: iterates a slice of connections per tick so heartbeat traffic is spread across the configured sweep window.
        /// Also detects timeouts and disconnects non-responsive peers.
        /// </summary>
        /// <returns>True if the connection is still valid. False will be returned if the connection had been disconnected.</returns>
        private bool PerformKeepAlive(long nowTicks, SynapseConnection synapseConnection, CancellationToken cancellationToken)
        {
            if (_transmissionEngine is null)
                return false;

            if (synapseConnection.State == ConnectionState.Disconnected)
                return false;

            // Timeout check - treated as a (benign) violation.
            // Default initial action is Kick (disconnect without blacklisting); a listener can escalate or downgrade.
            if (nowTicks - synapseConnection.LastReceivedTicks > _connectionTimeoutTicks)
            {
                synapseConnection.State = ConnectionState.Disconnected;
                Connections.Remove(synapseConnection.RemoteEndPoint, out _);
                ReturnReorderBufferToPool(synapseConnection);
                SynapseConnection.DrainPendingReliableQueue(synapseConnection);
                RaiseConnectionClosed(synapseConnection);
                HandleViolation(synapseConnection.RemoteEndPoint, synapseConnection.Signature, ViolationReason.Timeout, 0, null, ViolationAction.Kick);

                return false;
            }

            // Keep-alive: skip when traffic is already flowing; reset backoff when active.
            if (nowTicks - synapseConnection.LastReceivedTicks < _connectionKeepAliveTicks)
            {
                synapseConnection.UnansweredKeepAlives = 0;
                return true;
            }

            // Exponential backoff: double the interval for each consecutive unanswered keep-alive, capped at 8×.
            long effectiveIntervalTicks = _connectionKeepAliveTicks << Math.Min(synapseConnection.UnansweredKeepAlives, 3);

            if (nowTicks - synapseConnection.LastKeepAliveSentTicks < effectiveIntervalTicks)
                return true;

            // Track whether the previous keep-alive was answered.
            if (synapseConnection.LastKeepAliveSentTicks > 0 && synapseConnection.LastReceivedTicks < synapseConnection.LastKeepAliveSentTicks)
                synapseConnection.UnansweredKeepAlives++;
            else
                synapseConnection.UnansweredKeepAlives = 0;

            synapseConnection.LastKeepAliveSentTicks = nowTicks;

            _ = _transmissionEngine.SendKeepAliveAsync(synapseConnection, cancellationToken);

            return true;
        }

        /// <summary>
        /// Resets the per-connection inbound packet and byte counters for <paramref name="synapseConnection"/>
        /// if the one-second window has elapsed. No-op when both
        /// <see cref="SynapseSocket.Core.Configuration.SecurityConfig.MaximumPacketsPerSecond"/> and
        /// <see cref="SynapseSocket.Core.Configuration.SecurityConfig.MaximumBytesPerSecond"/> are disabled.
        /// </summary>
        private void ResetInboundRateCounters(long nowTicks, SynapseConnection synapseConnection)
        {
            if (_maximumPacketsPerSecond == SecurityConfig.DisabledMaximumPacketsPerSecond
                && _maximumBytesPerSecond == SecurityConfig.DisabledMaximumBytesPerSecond)
                return;

            synapseConnection.ResetInboundRateCounters(nowTicks);
        }
        
        /// <summary>
        /// Reliable retransmission sweep: any pending reliable packet whose resend timer has expired is re-sent.
        /// Packets exceeding the retry cap are treated as a <see cref="ViolationReason.ReliableExhausted"/> violation.
        /// </summary>
        private void RetransmitReliable(long nowTicks, SynapseConnection synapseConnection, CancellationToken cancellationToken)
        {
            if (_transmissionEngine is null)
                return;

            if (synapseConnection.State != ConnectionState.Connected)
                return;

            foreach (KeyValuePair<ushort, SynapseConnection.PendingReliable> keyValuePair in synapseConnection.PendingReliableQueue)
            {
                SynapseConnection.PendingReliable pendingReliable = keyValuePair.Value;

                if (nowTicks - pendingReliable.SentTicks < _reliableResendTicks)
                    continue;

                if (pendingReliable.Retries >= _maximumReliableRetries)
                {
                    synapseConnection.PendingReliableQueue.TryRemove(keyValuePair.Key, out _);
                    SynapseConnection.ReleasePendingReliable(pendingReliable);
                    Telemetry.OnLost();
                    HandleViolation(synapseConnection.RemoteEndPoint, synapseConnection.Signature, ViolationReason.ReliableExhausted, 0, ViolationReliableExhausted, ViolationAction.Kick);

                    return;
                }

                pendingReliable.Retries++;
                pendingReliable.SentTicks = nowTicks;

                Telemetry.OnReliableResend();

                for (int i = 0; i < pendingReliable.Segments.Count; i++)
                    _ = _transmissionEngine.SendRawAsync(pendingReliable.Segments[i], synapseConnection.RemoteEndPoint, cancellationToken);
            }
        }

        /// <summary>
        /// Evicts incomplete segment assemblies (reliable or unreliable) that have exceeded <see cref="SynapseSocket.Core.Configuration.SynapseConfig.SegmentAssemblyTimeoutMilliseconds"/> on each connection that has an active segmenter.
        /// </summary>
        private void TimeoutAssembledSegments(long nowTicks, SynapseConnection synapseConnection)
        {
            if (_segmentAssemblyTimeoutTicks == UnsetSegmentAssemblyTimeoutTicks)
                return;

            synapseConnection.Reassembler?.RemoveExpiredSegments(nowTicks, _segmentAssemblyTimeoutTicks);
        }
    }
}
