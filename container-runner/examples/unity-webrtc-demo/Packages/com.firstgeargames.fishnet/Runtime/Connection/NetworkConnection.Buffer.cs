using FishNet.Broadcast;
using FishNet.Managing;
using FishNet.Managing.Logging;
using FishNet.Managing.Transporting;
using FishNet.Object;
using FishNet.Transporting;
using System;
using System.Collections.Generic;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Connection
{
    public partial class NetworkConnection
    {
        #region Private.
        /// <summary>
        /// PacketBundles to send to this connection. An entry will be made for each channel.
        /// </summary>
        private List<PacketBundle> _toClientBundles = new();
        /// <summary>
        /// True if this object has been dirtied.
        /// </summary>
        private bool _serverDirtied;
        /// <summary>
        /// SplitReader being used as-needed.
        /// </summary>
        private SplitReader _splitReader;
        #endregion

        /// <summary>
        /// Initializes this script.
        /// </summary>
        private void InitializeBuffer()
        {
            for (byte i = 0; i < TransportManager.CHANNEL_COUNT; i++)
            {
                int mtu = NetworkManager.TransportManager.GetLowestMTU(i);
                _toClientBundles.Add(new(NetworkManager, mtu));
            }
        }

        /// <summary>
        /// Gets the current SplitReader.
        /// </summary>
        /// <returns></returns>
        internal bool TryGetSplitReader(int expectedMessages, out SplitReader splitReader)
        {
            if (_splitReader == null)
            {
                if (NetworkManager is null)
                {
                    NetworkManagerExtensions.LogError($"SplitReader cannot be returned because the NetworkManager reference is null.");
                    splitReader = null;

                    return false;
                }

                _splitReader = ResettableObjectCaches<SplitReader>.Retrieve();
                _splitReader.Initialize(NetworkManager, NetworkManager.TransportManager.MaximumClientPacketSize, isSenderClient: true, expectedMessages);
            }

            splitReader = _splitReader;

            return true;
        }

        /// <summary>
        /// Stores the current SpitReader on this connection.
        /// </summary>
        internal void StoreSplitReader() => ResettableObjectCaches<SplitReader>.StoreAndDefault(ref _splitReader);
        
        /// <summary>
        /// Sends a broadcast to this connection.
        /// </summary>
        /// <typeparam name = "T">Type of broadcast to send.</typeparam>
        /// <param name = "message">Broadcast data being sent; for example: an instance of your broadcast type.</param>
        /// <param name = "requireAuthenticated">True if the client must be authenticated for this broadcast to send.</param>
        /// <param name = "channel">Channel to send on.</param>
        public void Broadcast<T>(T message, bool requireAuthenticated = true, Channel channel = Channel.Reliable) where T : struct, IBroadcast
        {
            if (!IsActive)
                NetworkManager.LogError($"Connection is not valid, cannot send broadcast.");
            else
                NetworkManager.ServerManager.Broadcast(this, message, requireAuthenticated, channel);
        }

        /// <summary>
        /// Sends data from the server to a client.
        /// </summary>
        /// <param name = "forceNewBuffer">True to force data into a new buffer.</param>
        internal void SendToClient(byte channel, ArraySegment<byte> segment, bool forceNewBuffer = false, DataOrderType orderType = DataOrderType.Default)
        {
            // Cannot send data when disconnecting.
            if (Disconnecting)
                return;

            if (!IsActive)
            {
                NetworkManager.LogWarning($"Data cannot be sent to connection {ClientId} because it is not active.");
                return;
            }

            // If channel is out of bounds then default to the first channel.
            if (channel >= _toClientBundles.Count)
                channel = 0;

            _toClientBundles[channel].Write(segment, forceNewBuffer, orderType);
            ServerDirty();
        }

        /// <summary>
        /// Returns a PacketBundle for a channel. ResetPackets must be called afterwards.
        /// </summary>
        /// <param name = "channel"></param>
        /// <returns>True if PacketBundle is valid on the index and contains data.</returns>
        internal bool GetPacketBundle(int channel, out PacketBundle packetBundle)
        {
            return PacketBundle.GetPacketBundle(channel, _toClientBundles, out packetBundle);
        }

        /// <summary>
        /// Indicates the server has data to send to this connection.
        /// </summary>
        private void ServerDirty()
        {
            bool wasDirty = _serverDirtied;
            _serverDirtied = true;

            // If not yet dirty then tell transport manager this is dirty.
            if (!wasDirty)
                NetworkManager.TransportManager.ServerDirty(this);
        }

        /// <summary>
        /// Resets that there is data to send.
        /// </summary>
        internal void ResetServerDirty()
        {
            _serverDirtied = false;
        }
    }
}