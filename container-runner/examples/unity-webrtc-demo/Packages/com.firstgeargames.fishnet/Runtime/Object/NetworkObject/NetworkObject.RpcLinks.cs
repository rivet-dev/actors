using System.Collections.Generic;
using UnityEngine;

namespace FishNet.Object
{
    public partial class NetworkObject : MonoBehaviour
    {
        #region Private.
        /// <summary>
        /// RpcLinks being used within this NetworkObject.
        /// </summary>
        private List<ushort> _rpcLinkIndexes;
        #endregion

        /// <summary>
        /// Sets rpcLinkIndexes to values.
        /// </summary>
        internal void SetRpcLinkIndexes(List<ushort> values)
        {
            _rpcLinkIndexes = values;
        }

        /// <summary>
        /// Removes used link indexes from ClientObjects.
        /// </summary>
        internal void RemoveClientRpcLinkIndexes()
        {
            // if (NetworkManager != null)
            NetworkManager.ClientManager.Objects.RemoveLinkIndexes(_rpcLinkIndexes);
        }
    }
}