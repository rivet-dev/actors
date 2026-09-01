using System.Runtime.CompilerServices;
using FishNet.Connection;
using FishNet.Object.Synchronizing;
using FishNet.Serializing;
using FishNet.Serializing.Helping;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Object
{
    public partial class NetworkObject : MonoBehaviour
    {
        /// <summary>
        /// Writes SyncTypes for previous and new owner where permissions apply.
        /// </summary>
        private void WriteSyncTypesForManualOwnershipChange(NetworkConnection prevOwner)
        {
            if (prevOwner.IsActive)
                WriteForConnection(prevOwner, ReadPermission.ExcludeOwner);
            if (Owner.IsActive)
                WriteForConnection(Owner, ReadPermission.OwnerOnly);

            void WriteForConnection(NetworkConnection conn, ReadPermission permission)
            {
                for (int i = 0; i < NetworkBehaviours.Count; i++)
                    NetworkBehaviours[i].WriteSyncTypesForConnection(conn, permission);
            }
        }
    }
}