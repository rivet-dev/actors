using System;
using System.Net;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;
using SynapseSocket.Connections;
using SynapseSocket.Core.Configuration;
using SynapseSocket.Core.Events;

namespace SynapseSocket.Core
{

    /// <summary>
    /// NAT traversal support for <see cref="SynapseManager"/>.
    /// <para>
    /// <b>FullCone mode:</b> both peers already know each other's external endpoint — typically
    /// because they discovered each other through an out-of-band rendezvous service (see the
    /// <c>SynapseBeacon</c> sister project). <see cref="ConnectAsync"/> first waits
    /// <see cref="FullConeNatConfig.DirectAttemptMilliseconds"/> for a direct connection; if the
    /// connection is still pending it falls back to timed probe bursts that open NAT mappings on
    /// both sides simultaneously.
    /// </para>
    /// <para>
    /// Rendezvous/relay signaling itself is intentionally NOT implemented in SynapseSocket —
    /// external protocols should piggyback on the engine's UDP socket using the
    /// <see cref="SynapseManager.SendRawAsync"/> and <see cref="SynapseManager.UnknownPacketReceived"/>
    /// extension API so the NAT mapping opened by talking to the rendezvous service is the same
    /// mapping used for peer-to-peer traffic.
    /// </para>
    /// </summary>
    public sealed partial class SynapseManager
    {
        /// <summary>
        /// Sends timed probe bursts to open NAT mappings, followed by a handshake on each attempt.
        /// Raises <see cref="ConnectionFailed"/> with <see cref="ConnectionRejectedReason.NatTraversalFailed"/>
        /// if all attempts are exhausted without establishing a connection.
        /// </summary>
        private async Task NatPunchAsync(SynapseConnection synapseConnection, IPEndPoint endPoint, CancellationToken cancellationToken)
        {
            try
            {
                // FullCone: give the direct handshake a head start.
                if (Config.NatTraversal.Mode == NatTraversalMode.FullCone)
                {
                    await Task.Delay((int)Config.NatTraversal.FullCone.DirectAttemptMilliseconds, cancellationToken).ConfigureAwait(false);

                    if (synapseConnection.State != ConnectionState.Pending)
                        return;
                }

                for (uint attempt = 0; attempt < Config.NatTraversal.MaximumAttempts; attempt++)
                {
                    if (synapseConnection.State != ConnectionState.Pending)
                        return;

                    for (uint probe = 0; probe < Config.NatTraversal.ProbeCount; probe++)
                        await _transmissionEngine!.SendNatProbeAsync(endPoint, cancellationToken).ConfigureAwait(false);

                    await _transmissionEngine!.SendHandshakeAsync(endPoint, cancellationToken).ConfigureAwait(false);

                    if (attempt + 1 < Config.NatTraversal.MaximumAttempts)
                        await Task.Delay((int)Config.NatTraversal.IntervalMilliseconds, cancellationToken).ConfigureAwait(false);
                }

                if (synapseConnection.State != ConnectionState.Connected)
                    RaiseConnectionFailed(endPoint, ConnectionRejectedReason.NatTraversalFailed, null);
            }
            catch (OperationCanceledException) { }
            catch (ObjectDisposedException) { }
            catch (SocketException) { }
            catch (Exception unexpectedException)
            {
                UnhandledException?.Invoke(unexpectedException);
            }
        }
    }
}
