using FishNet.Connection;
using FishNet.Observing;
using UnityEngine;

namespace FishNet.Component.Observing
{
    /// <summary>
    /// This condition makes an object only visible to the owner.
    /// </summary>
    [CreateAssetMenu(menuName = "FishNet/Observers/Owner Only Condition", fileName = "New Owner Only Condition")]
    public class OwnerOnlyCondition : ObserverCondition
    {
        /// <summary>
        /// Returns if the object which this condition resides should be visible to connection.
        /// </summary>
        /// <param name = "connection">Connection which the condition is being checked for.</param>
        /// <param name = "currentlyAdded">True if the connection currently has visibility of this object.</param>
        /// <param name = "notProcessed">True if the condition was not processed. This can be used to skip processing for performance. While output as true this condition result assumes the previous ConditionMet value.</param>
        public override bool ConditionMet(NetworkConnection connection, bool currentlyAdded, out bool notProcessed)
        {
            notProcessed = false;
            /* Returning false immediately indicates no connection will
             * meet this condition. */
            return false;
        }

        /// <summary>
        /// How a condition is handled.
        /// </summary>
        /// <returns></returns>
        public override ObserverConditionType GetConditionType() => ObserverConditionType.Normal;
    }
}