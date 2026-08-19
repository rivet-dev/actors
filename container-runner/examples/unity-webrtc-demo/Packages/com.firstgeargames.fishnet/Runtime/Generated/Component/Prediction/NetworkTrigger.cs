namespace FishNet.Component.Prediction
{
    public sealed class NetworkTrigger : NetworkCollider
    {
        protected override void Awake()
        {
            IsTrigger = true;
            base.Awake();
        }
    }
}