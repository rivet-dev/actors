using FishNet.Connection;
using FishNet.Observing;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace FishNet.Component.Observing
{
    /// <summary>
    /// When this observer condition is placed on an object, a client must be within the same scene to view the object.
    /// </summary>
    [CreateAssetMenu(menuName = "FishNet/Observers/Scene Condition", fileName = "New Scene Condition")]
    public class SceneCondition : ObserverCondition
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

            if (NetworkObject == null || connection == null)
                return false;
            /* When there is no owner only then is the gameobject
             * scene checked. That's the only way to know at this point. */
            return connection.Scenes.Contains(NetworkObject.gameObject.scene);
        }

        /// <summary>
        /// How a condition is handled.
        /// </summary>
        /// <returns></returns>
        public override ObserverConditionType GetConditionType() => ObserverConditionType.Normal;
    }
}