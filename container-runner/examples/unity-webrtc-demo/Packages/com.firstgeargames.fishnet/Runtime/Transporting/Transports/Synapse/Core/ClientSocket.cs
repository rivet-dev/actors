using SynapseSocket.Connections;
using SynapseSocket.Core;
using SynapseSocket.Core.Configuration;
using SynapseSocket.Core.Events;
using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Net;
using System.Threading;
using System.Threading.Tasks;
using FishNet.Managing;

namespace FishNet.Transporting.Synapse.Client
{

    /// <summary>
    /// Manages the client-side Synapse connection to a remote server.
    /// </summary>
    public class ClientSocket : CommonSocket
    {
        ~ClientSocket()
        {
            StopConnection();
        }

        private string _serverAddress = string.Empty;
        private ushort _serverPort;
        private SynapseConfig? _pendingConfig;

        private SynapseManager? _manager;
        private SynapseConnection? _serverConnection;
        private IPEndPoint? _resolvedServerEndPoint;

        private ConcurrentQueue<Packet> _incomingPackets = new();
        private Queue<Packet> _outgoingPackets = new();

        /// <summary>
        /// Default connection timeout when none is specified.
        /// </summary>
        public const uint DefaultTimeoutMilliseconds = 15000;

        /// <summary>
        /// Initializes this socket for use.
        /// </summary>
        internal void Initialize(Transport transport)
        {
            Transport = transport;
        }

        /// <summary>
        /// Returns the server port if the socket is running, otherwise null.
        /// </summary>
        internal ushort? GetPort()
        {
            if (_manager is null || !_manager.IsRunning)
                return null;

            return _serverPort;
        }

        /// <summary>
        /// Starts a connection to the specified address and port.
        /// </summary>
        internal bool StartConnection(string address, ushort port, SynapseConfig config)
        {
            if (GetConnectionState() != LocalConnectionState.Stopped)
                StopSocket();

            LocalConnectionStates.Enqueue(LocalConnectionState.Starting);
            IterateIncoming();

            _pendingConfig = config;
            _serverAddress = address;
            _serverPort = port;
            ResetQueues();
            StartSocket();

            return true;
        }

        /// <summary>
        /// Stops the local client connection.
        /// </summary>
        internal bool StopConnection()
        {
            LocalConnectionState localConnectionState = GetConnectionState();

            if (localConnectionState == LocalConnectionState.Stopped || localConnectionState == LocalConnectionState.Stopping)
                return false;

            SetConnectionState(LocalConnectionState.Stopping, false);
            StopSocket();
            return true;
        }

        /// <summary>
        /// Processes queued incoming packets and connection state changes.
        /// </summary>
        internal void IterateIncoming()
        {
            while (LocalConnectionStates.TryDequeue(out LocalConnectionState localConnectionState))
                SetConnectionState(localConnectionState, false);

            LocalConnectionState state = GetConnectionState();

            if (state != LocalConnectionState.Started)
            {
                ResetQueues();

                if (state == LocalConnectionState.Stopped)
                {
                    StopSocket();
                    return;
                }
            }

            while (_incomingPackets.TryDequeue(out Packet packet))
            {
                ClientReceivedDataArgs dataArgs = new(packet.GetArraySegment(), (Channel)packet.Channel, Transport.Index);
                Transport.HandleClientReceivedDataArgs(dataArgs);
                packet.Dispose();
            }
        }

        /// <summary>
        /// Processes queued outgoing packets, sending each to the server.
        /// </summary>
        internal void IterateOutgoing()
        {
            SynapseManager? manager = _manager;
            SynapseConnection? serverConnection = _serverConnection;

            if (manager is null || serverConnection is null || GetConnectionState() != LocalConnectionState.Started)
            {
                ClearPacketQueue(ref _outgoingPackets);
                return;
            }

            int count = _outgoingPackets.Count;

            for (int i = 0; i < count; i++)
            {
                Packet outgoing = _outgoingPackets.Dequeue();
                bool isReliable = outgoing.Channel == (byte)Channel.Reliable;

                // Payload is copied inside SendAsync before the first await; safe to dispose after.
                _ = manager.SendAsync(serverConnection, outgoing.GetArraySegment(), isReliable, CancellationToken.None);
                outgoing.Dispose();
            }
        }

        /// <summary>
        /// Enqueues data to be sent to the server on the next IterateOutgoing call.
        /// </summary>
        internal void SendToServer(byte channelId, ArraySegment<byte> segment)
        {
            if (GetConnectionState() != LocalConnectionState.Started)
                return;

            Send(ref _outgoingPackets, channelId, segment, 0);
        }

        private void StartSocket()
        {
            if (!IPAddress.TryParse(_serverAddress, out IPAddress? ipAddress))
            {
                IPHostEntry hostEntry = Dns.GetHostEntry(_serverAddress);

                if (hostEntry.AddressList.Length == 0)
                {
                    Transport.NetworkManager.LogError($"Could not resolve server address: {_serverAddress}.");
                    StopSocket();
                    return;
                }

                ipAddress = hostEntry.AddressList[0];
            }

            _resolvedServerEndPoint = new(ipAddress, _serverPort);

            SynapseConfig synapseConfig = _pendingConfig!;
            synapseConfig.BindEndPoints = new() { new(IPAddress.Any, 0) };

            if (synapseConfig.Security.SignatureProviderAsset != null)
                synapseConfig.Security.SignatureProvider = synapseConfig.Security.SignatureProviderAsset;

            if (synapseConfig.Security.SignatureValidatorAsset != null)
                synapseConfig.Security.SignatureValidator = synapseConfig.Security.SignatureValidatorAsset;

            _manager = new(synapseConfig);
            _manager.PacketReceived += Manager_PacketReceived;
            _manager.ConnectionClosed += Manager_ConnectionClosed;
            _manager.ConnectionFailed += Manager_ConnectionFailed;
            _manager.UnhandledException += Manager_UnhandledException;

            _ = StartSocketAsync();
        }

        private async Task StartSocketAsync()
        {
            SynapseManager? manager = _manager;
            IPEndPoint? resolvedServerEndPoint = _resolvedServerEndPoint;

            if (manager is null || resolvedServerEndPoint is null)
                return;

            try
            {
                await manager.StartAsync(CancellationToken.None).ConfigureAwait(false);
                _serverConnection = await manager.ConnectAsync(resolvedServerEndPoint, CancellationToken.None).ConfigureAwait(false);
                LocalConnectionStates.Enqueue(LocalConnectionState.Started);
            }
            catch (Exception exception)
            {
                Transport.NetworkManager.LogError($"Client failed to connect to {_serverAddress}:{_serverPort} - {exception.Message}");
                StopSocket();
            }
        }

        private void StopSocket()
        {
            SynapseManager? manager = _manager;
            _manager = null;
            _serverConnection = null;
            _resolvedServerEndPoint = null;

            if (manager is not null)
                _ = manager.StopAsync();

            if (GetConnectionState() != LocalConnectionState.Stopped)
                LocalConnectionStates.Enqueue(LocalConnectionState.Stopped);
        }

        private void ResetQueues()
        {
            ClearGenericQueue(ref LocalConnectionStates);
            ClearPacketQueue(ref _incomingPackets);
            ClearPacketQueue(ref _outgoingPackets);
        }

        private void Manager_PacketReceived(PacketReceivedEventArgs packetReceivedEventArgs)
        {
            byte channel = packetReceivedEventArgs.IsReliable ? (byte)Channel.Reliable : (byte)Channel.Unreliable;
            Packet packet = new(0, packetReceivedEventArgs.Payload, channel);
            _incomingPackets.Enqueue(packet);
        }

        private void Manager_ConnectionClosed(ConnectionEventArgs connectionEventArgs)
        {
            // Ignore the event if we already initiated the stop.
            if (_manager is null)
                return;

            StopConnection();
        }

        private void Manager_ConnectionFailed(ConnectionFailedEventArgs connectionFailedEventArgs)
        {
            Transport.NetworkManager.LogWarning($"Client connection failed: {connectionFailedEventArgs.Reason} - {connectionFailedEventArgs.Message}");
            StopSocket();
        }

        private void Manager_UnhandledException(Exception exception)
        {
            Transport.NetworkManager.LogError($"Unhandled exception in Synapse client transport: {exception}");
        }
    }
}
