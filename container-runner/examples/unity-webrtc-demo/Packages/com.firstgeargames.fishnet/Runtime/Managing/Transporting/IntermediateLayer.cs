using System;
using UnityEngine;

namespace FishNet.Managing.Transporting
{
    /// <summary>
    /// When inherited from this may be used with the TransportManager to alter messages before they are sent and received.
    /// </summary>
    public abstract class IntermediateLayer : MonoBehaviour
    {
        /// <summary>
        /// TransportManager associated with this script.
        /// </summary>
        public TransportManager TransportManager { get; private set; }

        /// <summary>
        /// Called when data is received.
        /// </summary>
        /// <param name = "src">Original data.</param>
        /// <param name = "fromServer">True if receiving from the server, false if from a client.</param>
        /// <returns>Modified data.</returns>
        public abstract ArraySegment<byte> HandleIncoming(ArraySegment<byte> src, bool fromServer);

        /// <summary>
        /// Called when data is sent.
        /// </summary>
        /// <param name = "src">Original data.</param>
        /// <param name = "toServer">True if sending to the server, false if to a client.</param>
        /// <returns>Modified data.</returns>
        public abstract ArraySegment<byte> HandleOutgoing(ArraySegment<byte> src, bool toServer);

        /// <summary>
        /// Initializes this IntermediateLayer for use.
        /// </summary>
        public virtual void InitializeOnce(TransportManager manager) => TransportManager = manager;
    }
}