using UnityEngine;

namespace FishNet.Component.Prediction
{
    public sealed class NetworkCollision : NetworkCollider
    {
        protected override void Awake()
        {
            IsTrigger = false;
            base.Awake();
        }
    }
}