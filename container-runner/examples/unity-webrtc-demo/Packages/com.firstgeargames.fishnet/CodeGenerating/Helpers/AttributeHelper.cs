using FishNet.CodeGenerating.Helping.Extension;
using FishNet.Object;
using FishNet.Object.Helping;
using FishNet.Object.Prediction;
using MonoFN.Cecil;

namespace FishNet.CodeGenerating.Helping
{
    internal class AttributeHelper : CodegenBase
    {
        #region Reflection references.
        internal string ReplicateAttribute_FullName;
        internal string ReconcileAttribute_FullName;
        private string ServerAttribute_FullName;
        private string ClientAttribute_FullName;
        private string ServerRpcAttribute_FullName;
        private string ObserversRpcAttribute_FullName;
        private string TargetRpcAttribute_FullName;
        #endregion

        public override bool ImportReferences()
        {
            ServerAttribute_FullName = typeof(ServerAttribute).FullName;
            ClientAttribute_FullName = typeof(ClientAttribute).FullName;
            ServerRpcAttribute_FullName = typeof(ServerRpcAttribute).FullName;
            ObserversRpcAttribute_FullName = typeof(ObserversRpcAttribute).FullName;
            TargetRpcAttribute_FullName = typeof(TargetRpcAttribute).FullName;
            ReplicateAttribute_FullName = typeof(ReplicateAttribute).FullName;
            ReconcileAttribute_FullName = typeof(ReconcileAttribute).FullName;
            return true;
        }

        /// <summary>
        /// Returns type of Rpc attributeFullName is for.
        /// </summary>
        /// <param name = "attributeFullName"></param>
        /// <returns></returns>
        public RpcType GetRpcAttributeType(CustomAttribute ca)
        {
            if (ca.Is(ServerRpcAttribute_FullName))
                return RpcType.Server;
            else if (ca.Is(ObserversRpcAttribute_FullName))
                return RpcType.Observers;
            else if (ca.Is(TargetRpcAttribute_FullName))
                return RpcType.Target;
            else
                return RpcType.None;
        }

        /// <summary>
        /// Returns type of Rpc attributeFullName is for.
        /// </summary>
        /// <param name = "attributeFullName"></param>
        /// <returns></returns>
        internal QolAttributeType GetQolAttributeType(string attributeFullName)
        {
            if (attributeFullName == ServerAttribute_FullName)
                return QolAttributeType.Server;
            else if (attributeFullName == ClientAttribute_FullName)
                return QolAttributeType.Client;
            else
                return QolAttributeType.None;
        }
    }
}