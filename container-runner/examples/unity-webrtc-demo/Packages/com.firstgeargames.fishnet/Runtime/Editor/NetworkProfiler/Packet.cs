
using UnityEngine;

namespace FishNet.Editing.NetworkProfiler
{
    /// <summary>
    /// Information about a single packet.
    /// </summary>
    public struct Packet
    {
        /// <summary>
        /// Details about the packet, such as method or class name.
        /// </summary>
        /// <remarks>This may be empty.</remarks>
        public string Details;
        /// <summary>
        /// Bytes used.
        /// </summary>
        public ulong Bytes;
        /// <summary>
        /// Originating GameObject.
        /// </summary>
        /// <remarks>GameObject is used rather than a script reference because we do not want to risk unintentionally holding a script in memory. Unity will automatically clean up GameObjects, so they are safe to reference.</remarks>
        public GameObject GameObject;
            
        public Packet(ulong bytes) : this(details: string.Empty, bytes, gameObject: null) { }
        public Packet(string details, ulong bytes) : this(details, bytes, gameObject: null) { }
        public Packet(ulong bytes, GameObject gameObject) : this(details: string.Empty, bytes, gameObject) { }

        public Packet(string details, ulong bytes, GameObject gameObject)
        {
            Details = details;
            Bytes = bytes;
            GameObject = gameObject;
        }
    }

}