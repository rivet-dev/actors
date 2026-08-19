using FishNet.Managing;
using FishNet.Managing.Logging;
using UnityEngine;

namespace FishNet.Object
{
    public abstract partial class NetworkBehaviour : MonoBehaviour
    {
        /// <summary>
        /// True if can log for loggingType.
        /// </summary>
        /// <param name = "loggingType">Type of logging being filtered.</param>
        /// <returns></returns>
        public bool CanLog(LoggingType loggingType)
        {
            return NetworkManager.CanLog(loggingType);
        }
    }
}