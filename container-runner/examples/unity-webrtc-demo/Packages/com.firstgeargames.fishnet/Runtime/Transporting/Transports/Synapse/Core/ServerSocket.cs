using FishNet.Connection;
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

namespace FishNet.Transporting.Synapse.Server
{

    /// <summary>
    /// Manages the server-side Synapse transport, accepting and tracking remote clients.
    /// </summary>
    public class ServerSocket : CommonSocket
    {
        ~ServerSocket()
        {
            StopConnection();
        }

        private ushort _port;
        private string _ipv4BindAddress = string.Empty;
        private string _ipv6BindAddress = string.Empty;
        private int _maximumClients;
        private SynapseConfig? _pendingConfig;

        private SynapseManager? _manager;

        private ConcurrentDictionary<SynapseConnection, int> _connectionToId = new();
        private ConcurrentDictionary<int, SynapseConnection> _idToConnection = new();
        private int _nextConnectionId;

        private ConcurrentQueue<Packet> _incomingPackets = new();
        private Queue<Packet> _outgoingPackets = new();
        private ConcurrentQueue<RemoteConnectionEvent> _remoteConnectionEvents = new();

        /// <summary>
        /// Default connection timeout when none is specified.
        /// </summary>
        public const uint DefaultTimeoutMilliseconds = 15000;

        /// <summary>
        /// Returns the remote connection state for a given connection ID.
        /// </summary>
        internal RemoteConnectionState GetConnectionState(int connectionId)
        {
            if (!_idToConnection.TryGetValue(connectionId, out SynapseConnection? synapseConnection))
                return RemoteConnectionState.Stopped;

            return synapseConnection.State == ConnectionState.Connected
                ? RemoteConnectionState.Started
                : RemoteConnectionState.Stopped;
        }

        /// <summary>
        /// Returns the remote endpoint address string for a given connection ID.
        /// </summary>
        internal string GetConnectionAddress(int connectionId)
        {
            if (GetConnectionState() != LocalConnectionState.Started)
            {
                Transport.NetworkManager.LogWarning("Server socket is not started.");
                return string.Empty;
            }

            if (!_idToConnection.TryGetValue(connectionId, out SynapseConnection? synapseConnection))
            {
                Transport.NetworkManager.LogWarning($"Connection Id {connectionId} was not found.");
                return string.Empty;
            }

            return synapseConnection.RemoteEndPoint.ToString();
        }

        /// <summary>
        /// Returns the maximum number of clients the server accepts.
        /// </summary>
        internal int GetMaximumClients() =>
            Math.Min(_maximumClients, NetworkConnection.MAXIMUM_CLIENTID_WITHOUT_SIMULATED_VALUE);

        /// <summary>
        /// Updates the maximum number of clients at runtime.
        /// </summary>
        internal void SetMaximumClients(int value)
        {
            _maximumClients = value;
        }

        /// <summary>
        /// Initializes this socket for use.
        /// </summary>
        internal void Initialize(Transport transport)
        {
            Transport = transport;
        }

        /// <summary>
        /// Returns the listening port if the server is running, otherwise null.
        /// </summary>
        internal ushort? GetPort()
        {
            if (_manager is null || !_manager.IsRunning)
                return null;

            return _port;
        }

        /// <summary>
        /// Starts the server on the specified port and address.
        /// </summary>
        internal bool StartConnection(ushort port, int maximumClients, string ipv4BindAddress, string ipv6BindAddress, SynapseConfig config)
        {
            if (GetConnectionState() != LocalConnectionState.Stopped)
                StopSocket();

            LocalConnectionStates.Enqueue(LocalConnectionState.Starting);
            IterateIncoming();

            _pendingConfig = config;
            _port = port;
            _maximumClients = maximumClients;
            _ipv4BindAddress = ipv4BindAddress;
            _ipv6BindAddress = ipv6BindAddress;
            ResetQueues();
            StartSocket();

            return true;
        }

        /// <summary>
        /// Stops the local server.
        /// </summary>
        internal bool StopConnection()
        {
            if (_manager is null || (GetConnectionState() == LocalConnectionState.Stopped || GetConnectionState() == LocalConnectionState.Stopping))
                return false;

            LocalConnectionStates.Enqueue(LocalConnectionState.Stopping);
            StopSocket();
            return true;
        }

        /// <summary>
        /// Disconnects a specific remote client by connection ID.
        /// </summary>
        internal bool StopConnection(int connectionId)
        {
            if (GetConnectionState() != LocalConnectionState.Started || _manager is null)
                return false;

            if (!_idToConnection.TryGetValue(connectionId, out SynapseConnection? synapseConnection))
                return false;

            _ = _manager.DisconnectAsync(synapseConnection, CancellationToken.None);
            return true;
        }

        /// <summary>
        /// Processes queued incoming packets and remote connection events.
        /// </summary>
        internal void IterateIncoming()
        {
            while (LocalConnectionStates.TryDequeue(out LocalConnectionState localConnectionState))
                SetConnectionState(localConnectionState, true);

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

            while (_remoteConnectionEvents.TryDequeue(out RemoteConnectionEvent remoteConnectionEvent))
            {
                RemoteConnectionState remoteConnectionState = remoteConnectionEvent.IsConnected
                    ? RemoteConnectionState.Started
                    : RemoteConnectionState.Stopped;

                Transport.HandleRemoteConnectionState(new(remoteConnectionState, remoteConnectionEvent.ConnectionId, Transport.Index));
            }

            while (_incomingPackets.TryDequeue(out Packet packet))
            {
                if (_idToConnection.ContainsKey(packet.ConnectionId))
                {
                    ServerReceivedDataArgs dataArgs = new(packet.GetArraySegment(), (Channel)packet.Channel, packet.ConnectionId, Transport.Index);
                    Transport.HandleServerReceivedDataArgs(dataArgs);
                }

                packet.Dispose();
            }
        }

        /// <summary>
        /// Processes queued outgoing packets, sending each to its target connection.
        /// </summary>
        internal void IterateOutgoing()
        {
            SynapseManager? manager = _manager;

            if (manager is null || GetConnectionState() != LocalConnectionState.Started)
            {
                ClearPacketQueue(ref _outgoingPackets);
                return;
            }

            int count = _outgoingPackets.Count;

            for (int i = 0; i < count; i++)
            {
                Packet outgoing = _outgoingPackets.Dequeue();
                bool isReliable = outgoing.Channel == (byte)Channel.Reliable;

                if (outgoing.ConnectionId == NetworkConnection.UNSET_CLIENTID_VALUE)
                {
                    foreach (SynapseConnection synapseConnection in _connectionToId.Keys)
                        _ = manager.SendAsync(synapseConnection, outgoing.GetArraySegment(), isReliable, CancellationToken.None);
                }
                else if (_idToConnection.TryGetValue(outgoing.ConnectionId, out SynapseConnection? synapseConnection))
                {
                    _ = manager.SendAsync(synapseConnection, outgoing.GetArraySegment(), isReliable, CancellationToken.None);
                }

                outgoing.Dispose();
            }
        }

        /// <summary>
        /// Enqueues data to be sent to a specific client on the next IterateOutgoing call.
        /// </summary>
        internal void SendToClient(byte channelId, ArraySegment<byte> segment, int connectionId)
        {
            Send(ref _outgoingPackets, channelId, segment, connectionId);
        }

        private void StartSocket()
        {
            IPAddress ipv4Address;

            if (string.IsNullOrEmpty(_ipv4BindAddress))
            {
                ipv4Address = IPAddress.Any;
            }
            else if (!IPAddress.TryParse(_ipv4BindAddress, out ipv4Address!))
            {
                Transport.NetworkManager.LogError($"IPv4 bind address {_ipv4BindAddress} failed to parse. Clear the bind address field to use any bind address.");
                StopConnection();
                return;
            }

            List<IPEndPoint> bindEndPoints = new() { new(ipv4Address, _port) };

            if (!string.IsNullOrEmpty(_ipv6BindAddress))
            {
                if (!IPAddress.TryParse(_ipv6BindAddress, out IPAddress? ipv6Address))
                {
                    Transport.NetworkManager.LogError($"IPv6 bind address {_ipv6BindAddress} failed to parse. Clear the IPv6 bind address field to disable IPv6 binding.");
                    StopConnection();
                    return;
                }

                bindEndPoints.Add(new(ipv6Address, _port));
            }

            SynapseConfig synapseConfig = _pendingConfig!;
            synapseConfig.BindEndPoints = bindEndPoints;
            synapseConfig.MaximumConcurrentConnections = (uint)_maximumClients;

            if (synapseConfig.Security.SignatureProviderAsset != null)
                synapseConfig.Security.SignatureProvider = synapseConfig.Security.SignatureProviderAsset;

            if (synapseConfig.Security.SignatureValidatorAsset != null)
                synapseConfig.Security.SignatureValidator = synapseConfig.Security.SignatureValidatorAsset;

            _manager = new(synapseConfig);
            _manager.ConnectionEstablished += Manager_ConnectionEstablished;
            _manager.ConnectionClosed += Manager_ConnectionClosed;
            _manager.PacketReceived += Manager_PacketReceived;
            _manager.UnhandledException += Manager_UnhandledException;

            _ = StartSocketAsync();
        }

        private async Task StartSocketAsync()
        {
            SynapseManager? manager = _manager;

            if (manager is null)
                return;

            try
            {
                await manager.StartAsync(CancellationToken.None).ConfigureAwait(false);
                LocalConnectionStates.Enqueue(LocalConnectionState.Started);
            }
            catch (Exception exception)
            {
                Transport.NetworkManager.LogError($"Server failed to start: {exception.Message}");
                StopSocket();
            }
        }

        private void StopSocket()
        {
            SynapseManager? manager = _manager;
            _manager = null;

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
            ClearGenericQueue(ref _remoteConnectionEvents);
            _connectionToId.Clear();
            _idToConnection.Clear();
            _nextConnectionId = 0;
        }

        private void Manager_ConnectionEstablished(ConnectionEventArgs connectionEventArgs)
        {
            int connectionId = Interlocked.Increment(ref _nextConnectionId) - 1;
            _connectionToId[connectionEventArgs.Connection] = connectionId;
            _idToConnection[connectionId] = connectionEventArgs.Connection;
            _remoteConnectionEvents.Enqueue(new(true, connectionId));
        }

        private void Manager_ConnectionClosed(ConnectionEventArgs connectionEventArgs)
        {
            if (!_connectionToId.TryRemove(connectionEventArgs.Connection, out int connectionId))
                return;

            _idToConnection.TryRemove(connectionId, out _);
            _remoteConnectionEvents.Enqueue(new(false, connectionId));
        }

        private void Manager_PacketReceived(PacketReceivedEventArgs packetReceivedEventArgs)
        {
            if (!_connectionToId.TryGetValue(packetReceivedEventArgs.Connection, out int connectionId))
                return;

            byte channel = packetReceivedEventArgs.IsReliable ? (byte)Channel.Reliable : (byte)Channel.Unreliable;
            Packet packet = new(connectionId, packetReceivedEventArgs.Payload, channel);
            _incomingPackets.Enqueue(packet);
        }

        private void Manager_UnhandledException(Exception exception)
        {
            Transport.NetworkManager.LogError($"Unhandled exception in Synapse server transport: {exception}");
        }
    }
}
