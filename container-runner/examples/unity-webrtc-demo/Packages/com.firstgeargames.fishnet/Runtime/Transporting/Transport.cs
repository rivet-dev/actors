using FishNet.Managing;
using FishNet.Managing.Logging;
using System;
using UnityEngine;

namespace FishNet.Transporting
{
    /// <summary>
    /// Processes connection states, and data sent to and from a socket.
    /// </summary>
    public abstract class Transport : MonoBehaviour
    {
        #region Private.
        /// <summary>
        /// NetworkManager for this transport.
        /// </summary>
        public NetworkManager NetworkManager { get; private set; }
        /// <summary>
        /// Index this transport belongs to when using multiple transports at once.
        /// </summary>
        public int Index { get; private set; }
        #endregion

        #region Initialization and unity.
        /// <summary>
        /// Initializes the transport. Use this instead of Awake.
        /// <param name = "transportIndex">Index this transport belongs to when using multiple transports at once.</param>
        /// </summary>
        public virtual void Initialize(NetworkManager networkManager, int transportIndex)
        {
            NetworkManager = networkManager;
            Index = transportIndex;
        }
        #endregion

        #region ConnectionStates.
        /// <summary>
        /// Gets the address of a remote connection Id.
        /// </summary>
        /// <param name = "connectionId">Connectionid to get the address for.</param>
        /// <returns></returns>
        public abstract string GetConnectionAddress(int connectionId);

        /// <summary>
        /// Called when a connection state changes for the local client.
        /// </summary>
        public abstract event Action<ClientConnectionStateArgs> OnClientConnectionState;
        /// <summary>
        /// Called when a connection state changes for the local server.
        /// </summary>
        public abstract event Action<ServerConnectionStateArgs> OnServerConnectionState;
        /// <summary>
        /// Called when a connection state changes for a remote client.
        /// </summary>
        public abstract event Action<RemoteConnectionStateArgs> OnRemoteConnectionState;

        /// <summary>
        /// Handles a ConnectionStateArgs for the local client.
        /// </summary>
        /// <param name = "connectionStateArgs">Data being handled.</param>
        public abstract void HandleClientConnectionState(ClientConnectionStateArgs connectionStateArgs);

        /// <summary>
        /// Handles a ConnectionStateArgs for the local server.
        /// </summary>
        /// <param name = "connectionStateArgs">Data being handled.</param>
        public abstract void HandleServerConnectionState(ServerConnectionStateArgs connectionStateArgs);

        /// <summary>
        /// Handles a ConnectionStateArgs for a remote client.
        /// </summary>
        /// <param name = "connectionStateArgs">Data being handled.</param>
        public abstract void HandleRemoteConnectionState(RemoteConnectionStateArgs connectionStateArgs);

        /// <summary>
        /// Gets the current local ConnectionState.
        /// </summary>
        /// <param name = "server">True if getting ConnectionState for the server.</param>
        public abstract LocalConnectionState GetConnectionState(bool server);

        /// <summary>
        /// Gets the current ConnectionState of a client connected to the server. Can only be called on the server.
        /// </summary>
        /// <param name = "connectionId">ConnectionId to get ConnectionState for.</param>
        public abstract RemoteConnectionState GetConnectionState(int connectionId);
        #endregion

        #region Sending.
        /// <summary>
        /// Sends to the server.
        /// </summary>
        /// <param name = "channelId">Channel to use.</param>
        /// <param name = "segment">Data to send.</param>
        public abstract void SendToServer(byte channelId, ArraySegment<byte> segment);

        /// <summary>
        /// Sends to a client.
        /// </summary>
        /// <param name = "channelId">Channel to use.</param>
        /// <param name = "segment">Data to send.</param>
        /// <param name = "connectionId">ConnectionId to send to. When sending to clients can be used to specify which connection to send to.</param>
        public abstract void SendToClient(byte channelId, ArraySegment<byte> segment, int connectionId);
        #endregion

        #region Receiving
        /// <summary>
        /// Called when the client receives data.
        /// </summary>
        public abstract event Action<ClientReceivedDataArgs> OnClientReceivedData;

        /// <summary>
        /// Handles a ClientReceivedDataArgs.
        /// </summary>
        /// <param name = "receivedDataArgs">Data being handled.</param>
        public abstract void HandleClientReceivedDataArgs(ClientReceivedDataArgs receivedDataArgs);

        /// <summary>
        /// Called when the server receives data.
        /// </summary>
        public abstract event Action<ServerReceivedDataArgs> OnServerReceivedData;

        /// <summary>
        /// Handles a ServerReceivedDataArgs.
        /// </summary>
        /// <param name = "receivedDataArgs">Data being handled.</param>
        public abstract void HandleServerReceivedDataArgs(ServerReceivedDataArgs receivedDataArgs);

        /// <summary>
        /// Returns packet loss percentage. Not all transports support this feature.
        /// </summary>
        /// <param name = "asServer">True to return packet loss on the server, false to return packet loss on the client.</param>
        public virtual float GetPacketLoss(bool asServer) => 0f;
        #endregion

        #region Iterating.
        /// <summary>
        /// Processes data received by the socket.
        /// </summary>
        /// <param name = "asServer">True to read data from clients, false to read data from the server.
        public abstract void IterateIncoming(bool asServer);

        /// <summary>
        /// Processes data to be sent by the socket.
        /// </summary>
        /// <param name = "asServer">True to send data from the local server to clients, false to send from the local client to server.
        public abstract void IterateOutgoing(bool asServer);
        #endregion

        #region Configuration.
        /// <summary>
        /// Returns if the transport is only run locally, offline.
        /// While true several security checks are disabled.
        /// </summary>
        /// <param name = "connectionid">Optional connectionId to check against.</param>
        public virtual bool IsLocalTransport(int connectionid) => false;

        /// <summary>
        /// Gets how long in seconds until either the server or client socket must go without data before being timed out.
        /// </summary>
        /// <param name = "asServer">True to get the timeout for the server socket, false for the client socket.</param>
        /// <returns></returns>
        public virtual float GetTimeout(bool asServer) => -1f;

        /// <summary>
        /// Sets how long in seconds until either the server or client socket must go without data before being timed out.
        /// </summary>
        /// <param name = "asServer">True to set the timeout for the server socket, false for the client socket.</param>
        public virtual void SetTimeout(float value, bool asServer) { }

        /// <summary>
        /// Returns the maximum number of clients allowed to connect to the server. If the transport does not support this method the value -1 is returned.
        /// </summary>
        /// <returns>Maximum clients transport allows.</returns>
        public virtual int GetMaximumClients()
        {
            string message = $"The current transport does not support this feature.";
            NetworkManager.LogWarning(message);
            return -1;
        }

        /// <summary>
        /// Sets the maximum number of clients allowed to connect to the server. If applied at runtime and clients exceed this value existing clients will stay connected but new clients may not connect.
        /// </summary>
        /// <param name = "value">Maximum clients to allow.</param>
        public virtual void SetMaximumClients(int value)
        {
            string message = $"The current transport does not support this feature.";
            NetworkManager.LogWarning(message);
        }

        /// <summary>
        /// Sets which address the client will connect to.
        /// </summary>
        /// <param name = "address">Address client will connect to.</param>
        public virtual void SetClientAddress(string address) { }

        /// <summary>
        /// Returns which address the client will connect to.
        /// </summary>
        public virtual string GetClientAddress() => string.Empty;

        /// <summary>
        /// Sets which address the server will bind to.
        /// </summary>
        /// <param name = "address">Address server will bind to.</param>
        /// <param name = "addressType">Address type to set.</param>
        public virtual void SetServerBindAddress(string address, IPAddressType addressType) { }

        /// <summary>
        /// Gets which address the server will bind to.
        /// </summary>
        /// <param name = "addressType">Address type to return.</param>
        public virtual string GetServerBindAddress(IPAddressType addressType) => string.Empty;

        /// <summary>
        /// Sets which port to use.
        /// </summary>
        /// <param name = "port">Port to use.</param>
        public virtual void SetPort(ushort port) { }

        /// <summary>
        /// Gets which port to use.
        /// </summary>
        public virtual ushort GetPort() => 0;
        #endregion

        #region Start and stop.
        /// <summary>
        /// Starts the local server or client using configured settings.
        /// </summary>
        /// <param name = "server">True to start server.</param>
        public abstract bool StartConnection(bool server);

        /// <summary>
        /// Stops the local server or client.
        /// </summary>
        /// <param name = "server">True to stop server.</param>
        public abstract bool StopConnection(bool server);

        /// <summary>
        /// Stops a remote client from the server, disconnecting the client.
        /// </summary>
        /// <param name = "connectionId">ConnectionId of the client to disconnect.</param>
        /// <param name = "immediately">
        /// True to abrutly stop the client socket. The technique used to accomplish immediate disconnects may vary depending on the transport.
        /// When not using immediate disconnects it's recommended to perform disconnects using the ServerManager rather than accessing the transport directly.
        /// </param>
        public abstract bool StopConnection(int connectionId, bool immediately);

        /// <summary>
        /// Stops both client and server.
        /// </summary>
        public abstract void Shutdown();
        #endregion

        #region Channels.
        /// <summary>
        /// Gets the MTU for a channel.
        /// </summary>
        /// <param name = "channel">Channel to get MTU for.</param>
        /// <returns>MTU of channel.</returns>
        public abstract int GetMTU(byte channel);
        #endregion
    }
}