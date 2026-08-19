using System;
using System.Collections.Generic;
using FishNet.Connection;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Object
{
    public partial class NetworkObject : MonoBehaviour
    {
        /// <summary>
        /// Level of detail levels for observers.
        /// </summary>
        /// <remarks>This collection will be empty of this NetworkObject does not utilize level of detail.</remarks>
        internal Dictionary<NetworkConnection, uint> ObserverLevelOfDetailDivisors;
        /// <summary>
        /// True if level of detail has been initialized, indicating it can be used.
        /// </summary>
        internal bool ServerIsLevelOfDetailInitialized { get; private set; }
        /// <summary>
        /// True if LocalLevelOfDetailCalculationType is set to close only.
        /// </summary>
        internal bool IsLocalReconcileLODCloseObjectsOnly => _localLevelOfDetailCalculationType == LocalReconcileLODCalculationType.CloseObjectsOnly;
        /// <summary>
        /// How local reconciles are applied when using level of detail, specifically when the server had not sent a reconcile.
        /// </summary>
        [Tooltip("How local reconciles are applied when using level of detail calculations, specifically when the server had not sent a reconcile.")]
        [SerializeField]
        private LocalReconcileLODCalculationType _localLevelOfDetailCalculationType = LocalReconcileLODCalculationType.CloseObjectsOnly;
        /// <summary>
        /// True if to enable level of detail for this object. Level of detail supports prediction objects. This feature must be enabled on the ObserverManager to function.
        /// </summary>
        internal bool UseLevelOfDetail => _useLevelOfDetail;
        [Tooltip("True if to enable level of detail for this object. Level of detail supports prediction objects. This feature must be enabled on the ObserverManager to function.")]
        [SerializeField]
        private bool _useLevelOfDetail = false;
        /// <summary>
        /// True to use the same level of detail as the topmost parent NetworkObject. False to use a separate level of detail if nested.
        /// </summary>
        [Tooltip("True to use the same level of detail as the topmost parent NetworkObject. False to use a separate level of detail if nested.")]
        [SerializeField]
        private bool _useRootLevelOfDetail = true;
        /// <summary>
        /// Default level of detail index for new observers.
        /// </summary>
        internal const byte DEFAULT_LEVEL_OF_DETAIL_INDEX = 1;
        
        /// <summary>
        /// Updates the level of detail status based on current conditions.
        /// </summary>
        private void SetLevelOfDetailUsage()
        {
        }

        /// <summary>
        /// Called ObserversActive has changed.
        /// </summary>
        private void ObserversActiveChanged_LevelOfDetail()
        {
        }

        /// <summary>
        /// Clears observers for level of detail.
        /// </summary>
        private void ClearObserverLevelOfDetail()
        {
        }

        /// <summary>
        /// Adds an observer to level of detail if needed.
        /// </summary>
        /// <remarks>A connection is only added if this object supports level of detail.</remarks>
        private void AddObserverLevelOfDetail(NetworkConnection connection)
        {
        }

        /// <summary>
        /// Removes an observer from level of detail if needed.
        /// </summary>
        private void RemoveObserverLevelOfDetail(NetworkConnection connection)
        {
        }
    }
}