using FishNet.Managing;
using FishNet.Managing.Transporting;
using LiteNetLib.Layers;
using System;
using System.Runtime.CompilerServices;
using LiteNetLib;
using UnityEngine;

namespace FishNet.Transporting.Tugboat
{
    [DisallowMultipleComponent]
    [AddComponentMenu("FishNet/Transport/Tugboat")]
    public class Tugboat : Transport
    {
        ~Tugboat()
        {
            Shutdown();
        }

        #region Serialized.
        /* Settings / Misc. */
        /// <summary>
        /// True to stop local server and client sockets using a new thread.
        /// </summary>
        internal bool StopSocketsOnThread => _stopSocketsOnThread;
        [Tooltip("True to stop local server and client sockets using a new thread.")]
        [SerializeField]
        private bool _stopSocketsOnThread = false;
        /// <summary>
        /// While true, forces sockets to send data directly to interface without routing.
        /// </summary>
        internal bool DontRoute => _dontRoute;
        [Tooltip("While true, forces sockets to send data directly to interface without routing.")]
        [SerializeField]
        private bool _dontRoute;
        /// <summary>
        /// Allows the same address and port to be used multiple times by the server. This can be useful if you wish to launch multiple builds or server instances on the same machine using the same configuration.
        /// </summary>
        internal bool ReuseAddress => _reuseAddress;
        [Tooltip("Allows the same address and port to be used multiple times by the server. This can be useful if you wish to launch multiple builds or server instances on the same machine using the same configuration.")]
        [SerializeField]
        private bool _reuseAddress;
        /* Server. */
        /// <summary>
        /// IPv4 address to bind server to.
        /// </summary>
        [Tooltip("IPv4 Address to bind server to.")]
        [SerializeField]
        private string _ipv4BindAddress;
        /// <summary>
        /// Enable IPv6 only on demand to avoid problems in Linux environments where it may have been disabled on host
        /// </summary>
        [Tooltip("Enable IPv6, Server listens on IPv4 and IPv6 address")]
        [SerializeField]
        private bool _enableIpv6 = true;
        /// <summary>
        /// IPv6 address to bind server to.
        /// </summary>
        [Tooltip("IPv6 Address to bind server to.")]
        [SerializeField]
        private string _ipv6BindAddress;
        /// <summary>
        /// Port to use.
        /// </summary>
        [Tooltip("Port to use.")]
        [SerializeField]
        private ushort _port = 7770;
        /// <summary>
        /// Maximum number of players which may be connected at once.
        /// </summary>
        [Tooltip("Maximum number of players which may be connected at once.")]
        [Range(1, 9999)]
        [SerializeField]
        private int _maximumClients = 4095;
        /* Client. */
        /// <summary>
        /// Address to connect.
        /// </summary>
        [Tooltip("Address to connect.")]
        [SerializeField]
        private string _clientAddress = "localhost";
        #endregion

        #region Private.
        /// <summary>
        /// PacketLayer to use with LiteNetLib.
        /// </summary>
        private PacketLayerBase _packetLayer;
        /// <summary>
        /// Server socket and handler. This field is exposed for advanced-use. Use caution when accessing this outside of this class.
        /// </summary>
        public Server.ServerSocket ServerSocket = new();
        /// <summary>
        /// Client socket and handler. This field is exposed for advanced-use. Use caution when accessing this outside of this class.
        /// </summary>
        public Client.ClientSocket ClientSocket = new();
        /// <summary>
        /// Current timeout for the client.
        /// </summary>
        private int _clientTimeout = MAX_TIMEOUT_SECONDS;
        /// <summary>
        /// Current timeout for the server.
        /// </summary>
        private int _serverTimeout = MAX_TIMEOUT_SECONDS;
        #endregion

        #region Const.
        /// <summary>
        /// Maximum timeout value to use.
        /// </summary>
        private const ushort MAX_TIMEOUT_SECONDS = 1800;
        /// <summary>
        /// Maximum UDP packet size allowed.
        /// </summary>
        private const int MAXIMUM_UDP_MTU = 1350;
        #endregion

        #region Initialization and unity.
        public override void Initialize(NetworkManager networkManager, int transportIndex)
        {
            base.Initialize(networkManager, transportIndex);
            networkManager.TimeManager.OnUpdate += TimeManager_OnUpdate;
        }

        protected void OnDestroy()
        {
            Shutdown();
            if (NetworkManager != null)
                NetworkManager.TimeManager.OnUpdate -= TimeManager_OnUpdate;
        }
        #endregion

        #region ConnectionStates.
        /// <summary>
        /// Gets the address of a remote connection Id.
        /// </summary>
        /// <param name = "connectionId"></param>
        /// <returns></returns>
        public override string GetConnectionAddress(int connectionId)
        {
            return ServerSocket.GetConnectionAddress(connectionId);
        }

        /// <summary>
        /// Called when a connection state changes for the local client.
        /// </summary>
        public override event Action<ClientConnectionStateArgs> OnClientConnectionState;
        /// <summary>
        /// Called when a connection state changes for the local server.
        /// </summary>
        public override event Action<ServerConnectionStateArgs> OnServerConnectionState;
        /// <summary>
        /// Called when a connection state changes for a remote client.
        /// </summary>
        public override event Action<RemoteConnectionStateArgs> OnRemoteConnectionState;

        /// <summary>
        /// Gets the current local ConnectionState.
        /// </summary>
        /// <param name = "server">True if getting ConnectionState for the server.</param>
        public override LocalConnectionState GetConnectionState(bool server)
        {
            if (server)
                return ServerSocket.GetConnectionState();
            else
                return ClientSocket.GetConnectionState();
        }

        /// <summary>
        /// Gets the current ConnectionState of a remote client on the server.
        /// </summary>
        /// <param name = "connectionId">ConnectionId to get ConnectionState for.</param>
        public override RemoteConnectionState GetConnectionState(int connectionId)
        {
            return ServerSocket.GetConnectionState(connectionId);
        }

        /// <summary>
        /// Handles a ConnectionStateArgs for the local client.
        /// </summary>
        /// <param name = "connectionStateArgs"></param>
        public override void HandleClientConnectionState(ClientConnectionStateArgs connectionStateArgs)
        {
            OnClientConnectionState?.Invoke(connectionStateArgs);
        }

        /// <summary>
        /// Handles a ConnectionStateArgs for the local server.
        /// </summary>
        /// <param name = "connectionStateArgs"></param>
        public override void HandleServerConnectionState(ServerConnectionStateArgs connectionStateArgs)
        {
            OnServerConnectionState?.Invoke(connectionStateArgs);
        }

        /// <summary>
        /// Handles a ConnectionStateArgs for a remote client.
        /// </summary>
        /// <param name = "connectionStateArgs"></param>
        public override void HandleRemoteConnectionState(RemoteConnectionStateArgs connectionStateArgs)
        {
            OnRemoteConnectionState?.Invoke(connectionStateArgs);
        }
        #endregion

        #region Iterating.
        /// <summary>
        /// Called every update to poll for data.
        /// </summary>
        private void TimeManager_OnUpdate()
        {
            ServerSocket?.PollSocket();
            ClientSocket?.PollSocket();
        }

        /// <summary>
        /// Processes data received by the socket.
        /// </summary>
        /// <param name = "asServer">True to read data from clients, false to read data from the server.
        public override void IterateIncoming(bool asServer)
        {
            if (asServer)
                ServerSocket.IterateIncoming();
            else
                ClientSocket.IterateIncoming();
        }

        /// <summary>
        /// Processes data to be sent by the socket.
        /// </summary>
        /// <param name = "asServer">True to send data from the local server to clients, false to send from the local client to server.
        public override void IterateOutgoing(bool asServer)
        {
            if (asServer)
                ServerSocket.IterateOutgoing();
            else
                ClientSocket.IterateOutgoing();
        }
        #endregion

        #region Sending.
        /// <summary>
        /// Sends to the server or all clients.
        /// </summary>
        /// <param name = "channelId">Channel to use.</param>
        /// <param name = "segment">Data to send.</param>
        public override void SendToServer(byte channelId, ArraySegment<byte> segment)
        {
            SanitizeChannel(ref channelId);
            ClientSocket.SendToServer(channelId, segment);
        }

        /// <summary>
        /// Sends data to a client.
        /// </summary>
        /// <param name = "channelId"></param>
        /// <param name = "segment"></param>
        /// <param name = "connectionId"></param>
        public override void SendToClient(byte channelId, ArraySegment<byte> segment, int connectionId)
        {
            SanitizeChannel(ref channelId);
            ServerSocket.SendToClient(channelId, segment, connectionId);
        }
        #endregion

        #region Receiving.
        /// <summary>
        /// Called when client receives data.
        /// </summary>
        public override event Action<ClientReceivedDataArgs> OnClientReceivedData;

        /// <summary>
        /// Handles a ClientReceivedDataArgs.
        /// </summary>
        /// <param name = "receivedDataArgs"></param>
        public override void HandleClientReceivedDataArgs(ClientReceivedDataArgs receivedDataArgs)
        {
            OnClientReceivedData?.Invoke(receivedDataArgs);
        }

        /// <summary>
        /// Called when server receives data.
        /// </summary>
        public override event Action<ServerReceivedDataArgs> OnServerReceivedData;

        /// <summary>
        /// Handles a ClientReceivedDataArgs.
        /// </summary>
        /// <param name = "receivedDataArgs"></param>
        public override void HandleServerReceivedDataArgs(ServerReceivedDataArgs receivedDataArgs)
        {
            OnServerReceivedData?.Invoke(receivedDataArgs);
        }

        /// <summary>
        /// Returns packet loss percentage. This transport supports this feature.
        /// </summary>
        /// <param name = "asServer">True to return packet loss on the server, false to return packet loss on the client.</param>
        public override float GetPacketLoss(bool asServer)
        {
            NetManager nm;
            if (asServer && ServerSocket != null)
                nm = ServerSocket.NetManager;
            else if (!asServer && ClientSocket != null)
                nm = ClientSocket.NetManager;
            else
                nm = null;

            if (nm == null)
                return 0f;

            return nm.Statistics.PacketLossPercent;
        }
        #endregion

        #region Configuration.
        /// <summary>
        /// Sets which PacketLayer to use with LiteNetLib.
        /// </summary>
        /// <param name = "packetLayer"></param>
        public void SetPacketLayer(PacketLayerBase packetLayer)
        {
            _packetLayer = packetLayer;
            if (GetConnectionState(true) != LocalConnectionState.Stopped)
                NetworkManager.LogWarning("PacketLayer is set but will not be applied until the server stops.");
            if (GetConnectionState(false) != LocalConnectionState.Stopped)
                NetworkManager.LogWarning("PacketLayer is set but will not be applied until the client stops.");

            InitializeSocket(asServer: true);
            InitializeSocket(asServer: false);
        }

        /// <summary>
        /// How long in seconds until either the server or client socket must go without data before being timed out.
        /// </summary>
        /// <param name = "asServer">True to get the timeout for the server socket, false for the client socket.</param>
        /// <returns></returns>
        public override float GetTimeout(bool asServer)
        {
            // Server and client uses the same timeout.
            return (float)MAX_TIMEOUT_SECONDS;
        }

        /// <summary>
        /// Sets how long in seconds until either the server or client socket must go without data before being timed out.
        /// </summary>
        /// <param name = "asServer">True to set the timeout for the server socket, false for the client socket.</param>
        public override void SetTimeout(float value, bool asServer)
        {
            int timeoutValue = (int)Math.Ceiling(value);
            if (asServer)
                _serverTimeout = timeoutValue;
            else
                _clientTimeout = timeoutValue;

            UpdateTimeout();
        }

        /// <summary>
        /// Returns the maximum number of clients allowed to connect to the server. If the transport does not support this method the value -1 is returned.
        /// </summary>
        /// <returns></returns>
        public override int GetMaximumClients()
        {
            return ServerSocket.GetMaximumClients();
        }

        /// <summary>
        /// Sets maximum number of clients allowed to connect to the server. If applied at runtime and clients exceed this value existing clients will stay connected but new clients may not connect.
        /// </summary>
        /// <param name = "value"></param>
        public override void SetMaximumClients(int value)
        {
            _maximumClients = value;
            ServerSocket.SetMaximumClients(value);
        }

        /// <summary>
        /// Sets which address the client will connect to.
        /// </summary>
        /// <param name = "address"></param>
        public override void SetClientAddress(string address)
        {
            _clientAddress = address;
        }

        /// <summary>
        /// Gets which address the client will connect to.
        /// </summary>
        public override string GetClientAddress()
        {
            return _clientAddress;
        }

        /// <summary>
        /// Sets which address the server will bind to.
        /// </summary>
        /// <param name = "address"></param>
        public override void SetServerBindAddress(string address, IPAddressType addressType)
        {
            if (addressType == IPAddressType.IPv4)
                _ipv4BindAddress = address;
            else
                _ipv6BindAddress = address;
        }

        /// <summary>
        /// Gets which address the server will bind to.
        /// </summary>
        /// <param name = "address"></param>
        public override string GetServerBindAddress(IPAddressType addressType)
        {
            if (addressType == IPAddressType.IPv4)
                return _ipv4BindAddress;
            else
                return _ipv6BindAddress;
        }

        /// <summary>
        /// Sets which port to use.
        /// </summary>
        /// <param name = "port"></param>
        public override void SetPort(ushort port)
        {
            _port = port;
        }

        /// <summary>
        /// Gets which port to use.
        /// </summary>
        /// <param name = "port"></param>
        public override ushort GetPort()
        {
            // Server.
            ushort? result = ServerSocket?.GetPort();
            if (result.HasValue)
                return result.Value;
            // Client.
            result = ClientSocket?.GetPort();
            if (result.HasValue)
                return result.Value;

            return _port;
        }
        #endregion

        #region Start and stop.
        /// <summary>
        /// Starts the local server or client using configured settings.
        /// </summary>
        /// <param name = "server">True to start server.</param>
        public override bool StartConnection(bool server)
        {
            if (server)
                return StartServer();

            return StartClient(_clientAddress);
        }

        /// <summary>
        /// Stops the local server or client.
        /// </summary>
        /// <param name = "server">True to stop server.</param>
        public override bool StopConnection(bool server)
        {
            if (server)
                return StopServer();

            return StopClient();
        }

        /// <summary>
        /// Stops a remote client from the server, disconnecting the client.
        /// </summary>
        /// <param name = "connectionId">ConnectionId of the client to disconnect.</param>
        /// <param name = "immediately">
        /// True to abrutly stop the client socket. The technique used to accomplish immediate disconnects may vary depending on the transport.
        /// When not using immediate disconnects it's recommended to perform disconnects using the ServerManager rather than accessing the transport directly.
        /// </param>
        public override bool StopConnection(int connectionId, bool immediately)
        {
            return ServerSocket.StopConnection(connectionId);
        }

        /// <summary>
        /// Stops both client and server.
        /// </summary>
        public override void Shutdown()
        {
            // Stops client then server connections.
            StopConnection(false);
            StopConnection(true);
        }

        #region Privates.
        /// <summary>
        /// Initializes client or server socket.
        /// </summary>
        private void InitializeSocket(bool asServer)
        {
            if (asServer)
                ServerSocket.Initialize(this, MAXIMUM_UDP_MTU, _packetLayer, _enableIpv6);
            else
                ClientSocket.Initialize(this, MAXIMUM_UDP_MTU, _packetLayer);
        }

        /// <summary>
        /// Starts server.
        /// </summary>
        private bool StartServer()
        {
            InitializeSocket(asServer: true);
            UpdateTimeout();
            return ServerSocket.StartConnection(_port, _maximumClients, _ipv4BindAddress, _ipv6BindAddress);
        }

        /// <summary>
        /// Stops server.
        /// </summary>
        private bool StopServer()
        {
            if (ServerSocket == null)
                return false;

            return ServerSocket.StopConnection();
        }

        /// <summary>
        /// Starts the client.
        /// </summary>
        /// <param name = "address"></param>
        private bool StartClient(string address)
        {
            InitializeSocket(asServer: false);
            UpdateTimeout();
            return ClientSocket.StartConnection(address, _port);
        }

        /// <summary>
        /// Updates clients timeout values.
        /// </summary>
        private void UpdateTimeout()
        {
            ClientSocket.UpdateTimeout(_clientTimeout);
            ServerSocket.UpdateTimeout(_serverTimeout);
        }

        /// <summary>
        /// Stops the client.
        /// </summary>
        private bool StopClient()
        {
            if (ClientSocket == null)
                return false;

            return ClientSocket.StopConnection();
        }
        #endregion
        #endregion

        #region Channels.
        /// <summary>
        /// If channelId is invalid then channelId becomes forced to reliable.
        /// </summary>
        /// <param name = "channelId"></param>
        private void SanitizeChannel(ref byte channelId)
        {
            if (channelId >= TransportManager.CHANNEL_COUNT)
            {
                NetworkManager.LogWarning($"Channel of {channelId} is out of range of supported channels. Channel will be defaulted to reliable.");
                channelId = 0;
            }
        }

        /// <summary>
        /// Gets the MTU for a channel. This should take header size into consideration.
        /// For example, if MTU is 1200 and a packet header for this channel is 10 in size, this method should return 1190.
        /// </summary>
        /// <param name = "channel"></param>
        /// <returns></returns>
        public override int GetMTU(byte channel)
        {
            return MAXIMUM_UDP_MTU - NetConstants.MaxUdpHeaderSize;
        }
        #endregion
    }
}