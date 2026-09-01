using System.Collections.Generic;

namespace FishNet.Transporting.Yak
{
    public abstract class CommonSocket
    {
        #region Public.
        /// <summary>
        /// Current ConnectionState.
        /// </summary>
        private LocalConnectionState _connectionState = LocalConnectionState.Stopped;

        /// <summary>
        /// Returns the current ConnectionState.
        /// </summary>
        /// <returns></returns>
        internal LocalConnectionState GetLocalConnectionState()
        {
            return _connectionState;
        }

        #endregion

        #region Protected.
        /// <summary>
        /// Transport controlling this socket.
        /// </summary>
        protected Transport Transport = null;
        #endregion

        /// <summary>
        /// Initializes this for use.
        /// </summary>
        internal virtual void Initialize(Transport t, CommonSocket socket)
        {
            Transport = t;
        }

        /// <summary>
        /// Clears a queue.
        /// </summary>
        internal void ClearQueue(ref Queue<LocalPacket> queue)
        {
        }
    }
}