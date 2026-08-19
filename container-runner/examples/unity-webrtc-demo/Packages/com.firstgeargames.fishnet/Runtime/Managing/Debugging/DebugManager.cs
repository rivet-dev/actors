using UnityEngine;

namespace FishNet.Managing.Debugging
{
    /// <summary>
    /// A container for debugging.
    /// </summary>
    [DisallowMultipleComponent]
    [AddComponentMenu("FishNet/Manager/DebugManager")]
    public class DebugManager : MonoBehaviour
    {
        /// <summary>
        /// True to write additional information about scene objects being sent in spawn messages. This is primarily used to resolve sceneId not found errors.
        /// </summary>
        [Tooltip("True to write additional information about scene objects being sent in spawn messages. This is primarily used to resolve sceneId not found errors.")]
        public bool WriteSceneObjectDetails;
        /// <summary>
        /// True to validate written versus read length of Rpcs. Errors will be thrown if read length is not equal to written length.
        /// </summary>
        [Tooltip("True to validate written versus read length of Rpcs. Errors will be thrown if read length is not equal to written length.")]
        public bool ValidateRpcLengths;
        /// <summary>
        /// True to disable RpcLinks for Observer RPCs.
        /// </summary>
        [Tooltip("True to disable RpcLinks for Observer RPCs.")]
        public bool DisableObserversRpcLinks;
        /// <summary>
        /// True to disable RpcLinks for Target RPCs.
        /// </summary>
        [Tooltip("True to disable RpcLinks for Target RPCs.")]
        public bool DisableTargetRpcLinks;
        /// <summary>
        /// True to disable RpcLinks for Server RPCs.
        /// </summary>
        [Tooltip("True to disable RpcLinks for Server RPCs.")]
        public bool DisableServerRpcLinks;
        /// <summary>
        /// True to disable RpcLinks for Replicate RPCs.
        /// </summary>
        [Tooltip("True to disable RpcLinks for Replicate RPCs.")]
        public bool DisableReplicateRpcLinks;
        /// <summary>
        /// True to disable RpcLinks for Reconcile RPCs.
        /// </summary>
        [Tooltip("True to disable RpcLinks for Reconcile RPCs.")]
        public bool DisableReconcileRpcLinks;
    }
}