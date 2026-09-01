namespace FishNet.Managing.Scened
{
    public struct PreferredScene
    {
        /// <summary>
        /// Preferred scene for the client.
        /// </summary>
        public SceneLookupData Client;
        /// <summary>
        /// Preferred scene for the server.
        /// </summary>
        public SceneLookupData Server;

        /// <summary>
        /// Sets an individual preferred scene for client and server.
        /// </summary>
        public PreferredScene(SceneLookupData client, SceneLookupData server)
        {
            Client = client;
            Server = server;
        }

        /// <summary>
        /// Sets the same preferred scene for client and server.
        /// </summary>
        /// <param name = "sld"></param>
        public PreferredScene(SceneLookupData sld)
        {
            Client = sld;
            Server = sld;
        }
    }
}