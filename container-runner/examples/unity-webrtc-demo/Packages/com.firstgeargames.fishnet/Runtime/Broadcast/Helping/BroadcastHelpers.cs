// using FishNet.Connection; // remove on v5
// using FishNet.Serializing;
// using FishNet.Transporting;
// using GameKit.Dependencies.Utilities;
// using System;
// using System.Collections.Generic;
// using System.Diagnostics;

// namespace FishNet.Broadcast.Helping
// {
//    internal static class BroadcastHelper
//    {
//        /// <summary>
//        /// Gets the key for a broadcast type.
//        /// </summary>
//        /// <typeparam name="T"></typeparam>
//        /// <param name="broadcastType"></param>
//        /// <returns></returns>
//        internal static ushort GetKey<T>()
//        {
//            return typeof(T).FullName.GetStableHashU16();
//        }
//    }

//    /// <summary>
//    /// Implemented by server and client broadcast handlers.
//    /// </summary>
//    public abstract class BroadcastHandlerBase
//    {
//        /// <summary>
//        /// Current index when iterating invokes.
//        /// This value will be -1 when not iterating.
//        /// </summary>
//        protected int IteratingIndex;

//        public abstract void RegisterHandler(object obj);
//        public abstract void UnregisterHandler(object obj);
//        public virtual void InvokeHandlers(PooledReader reader, Channel channel) { }
//        public virtual void InvokeHandlers(NetworkConnection conn, PooledReader reader, Channel channel) { }
//        public virtual bool RequireAuthentication => false;
//    }

//    /// <summary>
//    /// Handles broadcasts received on server, from clients.
//    /// </summary>
//    internal class ClientBroadcastHandler<T> : BroadcastHandlerBase
//    {
//        /// <summary>
//        /// Action handlers for the broadcast.
//        /// </summary>
//        private List<Action<NetworkConnection, T, Channel>> _handlers = new List<Action<NetworkConnection, T, Channel>>();
//        /// <summary>
//        /// True to require authentication for the broadcast type.
//        /// </summary>
//        private bool _requireAuthentication;

//        public ClientBroadcastHandler(bool requireAuthentication)
//        {
//            _requireAuthentication = requireAuthentication;
//        }

//        /// <summary>
//        /// Invokes handlers after reading broadcast.
//        /// </summary>
//        /// <returns>True if a rebuild was required.</returns>
//        public override void InvokeHandlers(NetworkConnection conn, PooledReader reader, Channel channel)
//        {
//            T result = reader.Read<T>();
//            for (base.IteratingIndex = 0; base.IteratingIndex < _handlers.Count; base.IteratingIndex++)
//            {
//                Action<NetworkConnection, T, Channel> item = _handlers[base.IteratingIndex];
//                if (item != null)
//                {
//                    item.Invoke(conn, result, channel);
//                }
//                else
//                {
//                    _handlers.RemoveAt(base.IteratingIndex);
//                    base.IteratingIndex--;
//                }
//            }

//            base.IteratingIndex = -1;
//        }

//        /// <summary>
//        /// Adds a handler for this type.
//        /// </summary>
//        public override void RegisterHandler(object obj)
//        {
//            Action<NetworkConnection, T, Channel> handler = (Action<NetworkConnection, T, Channel>)obj;
//            _handlers.AddUnique(handler);
//        }

//        /// <summary>
//        /// Removes a handler from this type.
//        /// </summary>
//        /// <param name="handler"></param>
//        public override void UnregisterHandler(object obj)
//        {
//            Action<NetworkConnection, T, Channel> handler = (Action<NetworkConnection, T, Channel>)obj;
//            int indexOf = _handlers.IndexOf(handler);
//            // Not registered.
//            if (indexOf == -1)
//                return;

//            /* Has already been iterated over, need to subtract
//            * 1 from iteratingIndex to accomodate
//            * for the entry about to be removed. */
//            if (base.IteratingIndex >= 0 && (indexOf <= base.IteratingIndex))
//                base.IteratingIndex--;

//            // Remove entry.
//            _handlers.RemoveAt(indexOf);
//        }

//        /// <summary>
//        /// True to require authentication for the broadcast type.
//        /// </summary>
//        public override bool RequireAuthentication => _requireAuthentication;
//    }

//    /// <summary>
//    /// Handles broadcasts received on client, from server.
//    /// </summary>
//    internal class ServerBroadcastHandler<T> : BroadcastHandlerBase
//    {
//        /// <summary>
//        /// Action handlers for the broadcast.
//        /// Even though List lookups are slower this allows easy adding and removing of entries during iteration.
//        /// </summary>
//        private List<Action<T, Channel>> _handlers = new List<Action<T, Channel>>();

//        /// <summary>
//        /// Invokes handlers after reading broadcast.
//        /// </summary>
//        /// <returns>True if a rebuild was required.</returns>
//        public override void InvokeHandlers(PooledReader reader, Channel channel)
//        {
//            T result = reader.Read<T>();
//            for (base.IteratingIndex = 0; base.IteratingIndex < _handlers.Count; base.IteratingIndex++)
//            {
//                Action<T, Channel> item = _handlers[base.IteratingIndex];
//                if (item != null)
//                {
//                    item.Invoke(result, channel);
//                }
//                else
//                {
//                    _handlers.RemoveAt(base.IteratingIndex);
//                    base.IteratingIndex--;
//                }
//            }

//            base.IteratingIndex = -1;
//        }

//        /// <summary>
//        /// Adds a handler for this type.
//        /// </summary>
//        public override void RegisterHandler(object obj)
//        {
//            Action<T, Channel> handler = (Action<T, Channel>)obj;
//            _handlers.AddUnique(handler);
//        }

//        /// <summary>
//        /// Removes a handler from this type.
//        /// </summary>
//        /// <param name="handler"></param>
//        public override void UnregisterHandler(object obj)
//        {
//            Action<T, Channel> handler = (Action<T, Channel>)obj;
//            int indexOf = _handlers.IndexOf(handler);
//            // Not registered.
//            if (indexOf == -1)
//                return;

//            /* Has already been iterated over, need to subtract
//            * 1 from iteratingIndex to accomodate
//            * for the entry about to be removed. */
//            if (base.IteratingIndex >= 0 && (indexOf <= base.IteratingIndex))
//                base.IteratingIndex--;

//            //Remove entry.
//            _handlers.RemoveAt(indexOf);
//        }

//        /// <summary>
//        /// True to require authentication for the broadcast type.
//        /// </summary>
//        public override bool RequireAuthentication => false;
//    }

//}

