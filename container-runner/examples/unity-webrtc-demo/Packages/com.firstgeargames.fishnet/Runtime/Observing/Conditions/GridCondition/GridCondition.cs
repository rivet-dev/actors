using FishNet.Connection;
using FishNet.Observing;
using System.Runtime.CompilerServices;
using UnityEngine;

namespace FishNet.Component.Observing
{
    /// <summary>
    /// When this observer condition is placed on an object, a client must be within the specified grid accuracy to view the object.
    /// </summary>
    [CreateAssetMenu(menuName = "FishNet/Observers/Grid Condition", fileName = "New Grid Condition")]
    public class GridCondition : ObserverCondition
    {
        /// <summary>
        /// Returns if the object which this condition resides should be visible to connection.
        /// </summary>
        /// <param name = "connection">Connection which the condition is being checked for.</param>
        /// <param name = "currentlyAdded">True if the connection currently has visibility of this object.</param>
        /// <param name = "notProcessed">True if the condition was not processed. This can be used to skip processing for performance. While output as true this condition result assumes the previous ConditionMet value.</param>
        public override bool ConditionMet(NetworkConnection connection, bool currentlyAdded, out bool notProcessed)
        {
            // If here then checks are being processed.
            notProcessed = false;

            return connection.HashGridEntry.NearbyEntries.Contains(NetworkObject.HashGridEntry);
        }

        /// <summary>
        /// How a condition is handled.
        /// </summary>
        /// <returns></returns>
        public override ObserverConditionType GetConditionType() => ObserverConditionType.Timed;
    }
}