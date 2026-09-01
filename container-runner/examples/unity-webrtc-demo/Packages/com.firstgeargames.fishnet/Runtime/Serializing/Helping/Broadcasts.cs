using FishNet.Connection;
using FishNet.Managing;
using FishNet.Serializing;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;

namespace FishNet.Broadcast.Helping
{
    internal static class BroadcastsSerializers
    {
        /// <summary>
        /// Writes a broadcast to writer.
        /// </summary>
        internal static PooledWriter WriteBroadcast<T>(PooledWriter writer, T message)
        {
            writer.WritePacketIdUnpacked(PacketId.Broadcast);
            writer.WriteUInt16(typeof(T).FullName.GetStableHashU16());
            // Write data to a new writer.
            PooledWriter dataWriter = WriterPool.Retrieve();
            dataWriter.Write(message);
            // Write length of data.
            writer.WriteInt32(dataWriter.Length);
            // Write data.
            writer.WriteArraySegment(dataWriter.GetArraySegment());

            dataWriter.Store();

            return writer;
        }
    }

    internal static class BroadcastExtensions
    {
        /// <summary>
        /// Gets the key for a broadcast type.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <param name = "broadcastType"></param>
        /// <returns></returns>
        internal static ushort GetKey<T>()
        {
            return typeof(T).FullName.GetStableHashU16();
        }
    }

    /// <summary>
    /// Implemented by server and client broadcast handlers.
    /// </summary>
    public abstract class BroadcastHandlerBase
    {
        /// <summary>
        /// Current index when iterating invokes.
        /// This value will be -1 when not iterating.
        /// </summary>
        protected int IteratingIndex;
        public abstract void RegisterHandler(object obj);
        public abstract void UnregisterHandler(object obj);
        public virtual void InvokeHandlers(PooledReader reader, Channel channel) { }
        public virtual void InvokeHandlers(NetworkConnection conn, PooledReader reader, Channel channel) { }
        public virtual bool RequireAuthentication => false;
    }

    /// <summary>
    /// Handles broadcasts received on server, from clients.
    /// </summary>
    internal class ClientBroadcastHandler<T> : BroadcastHandlerBase
    {
        /// <summary>
        /// Action handlers for the broadcast.
        /// </summary>
        private List<Action<NetworkConnection, T, Channel>> _handlers = new();
        /// <summary>
        /// True to require authentication for the broadcast type.
        /// </summary>
        private bool _requireAuthentication;

        public ClientBroadcastHandler(bool requireAuthentication)
        {
            _requireAuthentication = requireAuthentication;
        }

        /// <summary>
        /// Invokes handlers after reading broadcast.
        /// </summary>
        /// <returns>True if a rebuild was required.</returns>
        public override void InvokeHandlers(NetworkConnection conn, PooledReader reader, Channel channel)
        {
            T result = reader.Read<T>();
            for (IteratingIndex = 0; IteratingIndex < _handlers.Count; IteratingIndex++)
            {
                Action<NetworkConnection, T, Channel> item = _handlers[IteratingIndex];
                if (item != null)
                {
                    item.Invoke(conn, result, channel);
                }
                else
                {
                    _handlers.RemoveAt(IteratingIndex);
                    IteratingIndex--;
                }
            }

            IteratingIndex = -1;
        }

        /// <summary>
        /// Adds a handler for this type.
        /// </summary>
        public override void RegisterHandler(object obj)
        {
            Action<NetworkConnection, T, Channel> handler = (Action<NetworkConnection, T, Channel>)obj;
            _handlers.AddUnique(handler);
        }

        /// <summary>
        /// Removes a handler from this type.
        /// </summary>
        /// <param name = "handler"></param>
        public override void UnregisterHandler(object obj)
        {
            Action<NetworkConnection, T, Channel> handler = (Action<NetworkConnection, T, Channel>)obj;
            int indexOf = _handlers.IndexOf(handler);
            // Not registered.
            if (indexOf == -1)
                return;

            /* Has already been iterated over, need to subtract
             * 1 from iteratingIndex to accomodate
             * for the entry about to be removed. */
            if (IteratingIndex >= 0 && indexOf <= IteratingIndex)
                IteratingIndex--;

            // Remove entry.
            _handlers.RemoveAt(indexOf);
        }

        /// <summary>
        /// True to require authentication for the broadcast type.
        /// </summary>
        public override bool RequireAuthentication => _requireAuthentication;
    }

    /// <summary>
    /// Handles broadcasts received on client, from server.
    /// </summary>
    internal class ServerBroadcastHandler<T> : BroadcastHandlerBase
    {
        /// <summary>
        /// Action handlers for the broadcast.
        /// Even though List lookups are slower this allows easy adding and removing of entries during iteration.
        /// </summary>
        private List<Action<T, Channel>> _handlers = new();

        /// <summary>
        /// Invokes handlers after reading broadcast.
        /// </summary>
        /// <returns>True if a rebuild was required.</returns>
        public override void InvokeHandlers(PooledReader reader, Channel channel)
        {
            T result = reader.Read<T>();
            for (IteratingIndex = 0; IteratingIndex < _handlers.Count; IteratingIndex++)
            {
                Action<T, Channel> item = _handlers[IteratingIndex];
                if (item != null)
                {
                    item.Invoke(result, channel);
                }
                else
                {
                    _handlers.RemoveAt(IteratingIndex);
                    IteratingIndex--;
                }
            }

            IteratingIndex = -1;
        }

        /// <summary>
        /// Adds a handler for this type.
        /// </summary>
        public override void RegisterHandler(object obj)
        {
            Action<T, Channel> handler = (Action<T, Channel>)obj;
            _handlers.AddUnique(handler);
        }

        /// <summary>
        /// Removes a handler from this type.
        /// </summary>
        /// <param name = "handler"></param>
        public override void UnregisterHandler(object obj)
        {
            Action<T, Channel> handler = (Action<T, Channel>)obj;
            int indexOf = _handlers.IndexOf(handler);
            // Not registered.
            if (indexOf == -1)
                return;

            /* Has already been iterated over, need to subtract
             * 1 from iteratingIndex to accomodate
             * for the entry about to be removed. */
            if (IteratingIndex >= 0 && indexOf <= IteratingIndex)
                IteratingIndex--;

            // Remove entry.
            _handlers.RemoveAt(indexOf);
        }

        /// <summary>
        /// True to require authentication for the broadcast type.
        /// </summary>
        public override bool RequireAuthentication => false;
    }
}