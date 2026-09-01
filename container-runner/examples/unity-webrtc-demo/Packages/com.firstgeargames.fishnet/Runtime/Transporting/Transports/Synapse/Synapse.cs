using FishNet.Managing;
using FishNet.Managing.Transporting;
using System;
using SynapseSocket.Core.Configuration;
using UnityEngine;

namespace FishNet.Transporting.Synapse
{

    /// <summary>
    /// FishNet transport backed by the SynapseSocket UDP networking library.
    /// </summary>
    [DisallowMultipleComponent]
    [AddComponentMenu("FishNet/Transport/Synapse")]
    public class Synapse : Transport
    {
        ~Synapse()
        {
            Shutdown();
        }

        /* Server. */
        /// <summary>
        /// IPv4 address to bind the server to. Leave empty to bind to all interfaces.
        /// </summary>
        [Tooltip("IPv4 address to bind the server to. Leave empty to bind to all interfaces.")]
        [SerializeField]
        private string _ipv4BindAddress = string.Empty;

        /// <summary>
        /// IPv6 address to bind the server to. Leave empty to disable IPv6 binding.
        /// </summary>
        [Tooltip("IPv6 address to bind the server to. Leave empty to disable IPv6 binding.")]
        [SerializeField]
        private string _ipv6BindAddress = string.Empty;

        /// <summary>
        /// Port used by both server and client.
        /// </summary>
        [Tooltip("Port used by both server and client.")]
        [SerializeField]
        private ushort _port = 7770;

        /// <summary>
        /// Maximum number of clients that may be connected at once.
        /// </summary>
        [Tooltip("Maximum number of clients that may be connected at once.")]
        [Range(1, 9999)]
        [SerializeField]
        private int _maximumClients = 4095;

        /* Client. */
        /// <summary>
        /// Address the client will connect to.
        /// </summary>
        [Tooltip("Address the client will connect to.")]
        [SerializeField]
        private string _clientAddress = "localhost";

        /// <summary>
        /// Advanced Synapse configuration applied when starting the server or client.
        /// </summary>
        [Tooltip("Advanced Synapse configuration applied when starting the server or client.")]
        [SerializeField]
        private SynapseConfig _synapseConfig = new();

        /// <summary>
        /// Server socket. Exposed for advanced use; access with caution outside this class.
        /// </summary>
        public Server.ServerSocket ServerSocket = new();

        /// <summary>
        /// Client socket. Exposed for advanced use; access with caution outside this class.
        /// </summary>
        public Client.ClientSocket ClientSocket = new();

        /// <summary>
        /// MTU returned to FishNet for packet sizing decisions.
        /// Synapse handles segmentation internally, so this reflects the per-segment wire limit.
        /// </summary>
        private const int MaximumTransmissionUnit = 1200;

        #region Initialization.
        /// <summary>
        /// Called by FishNet to initialize this transport.
        /// </summary>
        public override void Initialize(NetworkManager networkManager, int transportIndex)
        {
            base.Initialize(networkManager, transportIndex);
        }

        private void OnDestroy()
        {
            Shutdown();
        }
        #endregion

        #region Connection states.
        /// <summary>
        /// Called when the local client connection state changes.
        /// </summary>
        public override event Action<ClientConnectionStateArgs> OnClientConnectionState;

        /// <summary>
        /// Called when the local server connection state changes.
        /// </summary>
        public override event Action<ServerConnectionStateArgs> OnServerConnectionState;

        /// <summary>
        /// Called when a remote client connection state changes.
        /// </summary>
        public override event Action<RemoteConnectionStateArgs> OnRemoteConnectionState;

        /// <summary>
        /// Returns the local connection state for either the server or client.
        /// </summary>
        public override LocalConnectionState GetConnectionState(bool isServer)
        {
            if (isServer)
                return ServerSocket.GetConnectionState();
            else
                return ClientSocket.GetConnectionState();
        }

        /// <summary>
        /// Returns the connection state of a remote client on the server.
        /// </summary>
        public override RemoteConnectionState GetConnectionState(int connectionId)
        {
            return ServerSocket.GetConnectionState(connectionId);
        }

        /// <summary>
        /// Returns the remote endpoint address for a given connection ID.
        /// </summary>
        public override string GetConnectionAddress(int connectionId)
        {
            return ServerSocket.GetConnectionAddress(connectionId);
        }

        /// <summary>
        /// Forwards a client connection state change to subscribers.
        /// </summary>
        public override void HandleClientConnectionState(ClientConnectionStateArgs clientConnectionStateArgs)
        {
            OnClientConnectionState?.Invoke(clientConnectionStateArgs);
        }

        /// <summary>
        /// Forwards a server connection state change to subscribers.
        /// </summary>
        public override void HandleServerConnectionState(ServerConnectionStateArgs serverConnectionStateArgs)
        {
            OnServerConnectionState?.Invoke(serverConnectionStateArgs);
        }

        /// <summary>
        /// Forwards a remote client connection state change to subscribers.
        /// </summary>
        public override void HandleRemoteConnectionState(RemoteConnectionStateArgs remoteConnectionStateArgs)
        {
            OnRemoteConnectionState?.Invoke(remoteConnectionStateArgs);
        }
        #endregion

        #region Iterating.
        /// <summary>
        /// Processes data received by the socket.
        /// </summary>
        public override void IterateIncoming(bool isServer)
        {
            if (isServer)
                ServerSocket.IterateIncoming();
            else
                ClientSocket.IterateIncoming();
        }

        /// <summary>
        /// Processes data to be sent by the socket.
        /// </summary>
        public override void IterateOutgoing(bool isServer)
        {
            if (isServer)
                ServerSocket.IterateOutgoing();
            else
                ClientSocket.IterateOutgoing();
        }
        #endregion

        #region Sending.
        /// <summary>
        /// Sends data from the local client to the server.
        /// </summary>
        public override void SendToServer(byte channelId, ArraySegment<byte> segment)
        {
            SanitizeChannel(ref channelId);
            ClientSocket.SendToServer(channelId, segment);
        }

        /// <summary>
        /// Sends data from the server to a specific client.
        /// </summary>
        public override void SendToClient(byte channelId, ArraySegment<byte> segment, int connectionId)
        {
            SanitizeChannel(ref channelId);
            ServerSocket.SendToClient(channelId, segment, connectionId);
        }
        #endregion

        #region Receiving.
        /// <summary>
        /// Called when the client receives data.
        /// </summary>
        public override event Action<ClientReceivedDataArgs> OnClientReceivedData;

        /// <summary>
        /// Forwards received client data to subscribers.
        /// </summary>
        public override void HandleClientReceivedDataArgs(ClientReceivedDataArgs clientReceivedDataArgs)
        {
            OnClientReceivedData?.Invoke(clientReceivedDataArgs);
        }

        /// <summary>
        /// Called when the server receives data.
        /// </summary>
        public override event Action<ServerReceivedDataArgs> OnServerReceivedData;

        /// <summary>
        /// Forwards received server data to subscribers.
        /// </summary>
        public override void HandleServerReceivedDataArgs(ServerReceivedDataArgs serverReceivedDataArgs)
        {
            OnServerReceivedData?.Invoke(serverReceivedDataArgs);
        }
        #endregion

        #region Configuration.
        /// <summary>
        /// Returns how long in seconds until a connection times out.
        /// </summary>
        public override float GetTimeout(bool isServer)
        {
            return _synapseConfig.Connection.TimeoutMilliseconds / 1000f;
        }

        /// <summary>
        /// Sets how long in seconds until a connection times out.
        /// Takes effect on the next connection start.
        /// </summary>
        public override void SetTimeout(float value, bool isServer)
        {
            _synapseConfig.Connection.TimeoutMilliseconds = (uint)(Math.Max(0f, value) * 1000f);
        }

        /// <summary>
        /// Returns the maximum number of clients allowed to connect.
        /// </summary>
        public override int GetMaximumClients()
        {
            return ServerSocket.GetMaximumClients();
        }

        /// <summary>
        /// Sets the maximum number of clients allowed to connect.
        /// </summary>
        public override void SetMaximumClients(int value)
        {
            _maximumClients = value;
            ServerSocket.SetMaximumClients(value);
        }

        /// <summary>
        /// Sets the address the client will connect to.
        /// </summary>
        public override void SetClientAddress(string address)
        {
            _clientAddress = address;
        }

        /// <summary>
        /// Returns the address the client will connect to.
        /// </summary>
        public override string GetClientAddress()
        {
            return _clientAddress;
        }

        /// <summary>
        /// Sets the address the server will bind to.
        /// </summary>
        public override void SetServerBindAddress(string address, IPAddressType addressType)
        {
            if (addressType == IPAddressType.IPv4)
                _ipv4BindAddress = address;
            else
                _ipv6BindAddress = address;
        }

        /// <summary>
        /// Returns the address the server will bind to.
        /// </summary>
        public override string GetServerBindAddress(IPAddressType addressType)
        {
            return addressType == IPAddressType.IPv4 ? _ipv4BindAddress : _ipv6BindAddress;
        }

        /// <summary>
        /// Sets the port used by both server and client.
        /// </summary>
        public override void SetPort(ushort port)
        {
            _port = port;
        }

        /// <summary>
        /// Returns the active port. Prefers the server port, then the client port, then the configured value.
        /// </summary>
        public override ushort GetPort()
        {
            ushort? serverPort = ServerSocket?.GetPort();
            if (serverPort.HasValue)
                return serverPort.Value;

            ushort? clientPort = ClientSocket?.GetPort();
            if (clientPort.HasValue)
                return clientPort.Value;

            return _port;
        }
        #endregion

        #region Start and stop.
        /// <summary>
        /// Starts the local server or client.
        /// </summary>
        public override bool StartConnection(bool isServer)
        {
            if (isServer)
                return StartServer();

            return StartClient(_clientAddress);
        }

        /// <summary>
        /// Stops the local server or client.
        /// </summary>
        public override bool StopConnection(bool isServer)
        {
            if (isServer)
                return StopServer();

            return StopClient();
        }

        /// <summary>
        /// Stops a remote client from the server.
        /// </summary>
        public override bool StopConnection(int connectionId, bool immediately)
        {
            return ServerSocket.StopConnection(connectionId);
        }

        /// <summary>
        /// Stops both the server and client.
        /// </summary>
        public override void Shutdown()
        {
            StopConnection(false);
            StopConnection(true);
        }

        private bool StartServer()
        {
            ServerSocket.Initialize(this);
            return ServerSocket.StartConnection(_port, _maximumClients, _ipv4BindAddress, _ipv6BindAddress, _synapseConfig);
        }

        private bool StopServer()
        {
            if (ServerSocket is null)
                return false;

            return ServerSocket.StopConnection();
        }

        private bool StartClient(string address)
        {
            ClientSocket.Initialize(this);
            return ClientSocket.StartConnection(address, _port, _synapseConfig);
        }

        private bool StopClient()
        {
            if (ClientSocket is null)
                return false;

            return ClientSocket.StopConnection();
        }

        #endregion

        #region Channels.
        /// <summary>
        /// Clamps an invalid channel ID to reliable (0).
        /// </summary>
        private void SanitizeChannel(ref byte channelId)
        {
            if (channelId >= TransportManager.CHANNEL_COUNT)
            {
                NetworkManager.LogWarning($"Channel {channelId} is out of range. Defaulting to reliable.");
                channelId = 0;
            }
        }

        /// <summary>
        /// Returns the MTU for the given channel.
        /// Synapse handles segmentation internally; this value guides FishNet's packet sizing.
        /// </summary>
        public override int GetMTU(byte channel)
        {
            return MaximumTransmissionUnit;
        }
        #endregion
    }
}
