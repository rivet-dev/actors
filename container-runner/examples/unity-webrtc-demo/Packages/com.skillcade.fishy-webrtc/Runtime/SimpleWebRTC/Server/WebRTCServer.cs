#if !UNITY_WEBGL || UNITY_EDITOR

using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using Unity.WebRTC;
using UnityEngine;
using ConnectionId = System.UInt16;

namespace cakeslice.SimpleWebRTC
{
    public class WebRTCServer
    {
        public readonly ConcurrentQueue<Message> receiveQueue = new ConcurrentQueue<Message>();
        readonly int maxMessageSize;

        public HttpListener listener;
        Thread acceptThread;
        string allowedOrigin;
        bool suppressCorsHeaders;
        bool serverStopped;
        readonly BufferPool bufferPool;
        readonly ConcurrentDictionary<int, Connection> connections = new ConcurrentDictionary<int, Connection>();
        private ConcurrentQueue<ConnectionId> _idCache = new ConcurrentQueue<ConnectionId>();
        private ConnectionId _nextId = 0;

        private ConnectionId GetNextId()
        {
            if (_idCache.Count == 0)
                GrowIdCache(1000);

            ConnectionId result;
            _idCache.TryDequeue(out result);
            return result;
        }
        /// <summary>
        /// Grows IdCache by value.
        /// </summary>
        private void GrowIdCache(ConnectionId value)
        {
            ConnectionId over = (ConnectionId)((_nextId + value) - ConnectionId.MaxValue);
            //Prevent overflow.
            if (over > 0)
                value -= over;

            for (ConnectionId i = _nextId; i < value; i++)
                _idCache.Enqueue(i);
        }

        public WebRTCServer(int maxMessageSize, BufferPool bufferPool)
        {
            // TODO: SSL Support (sslConfig)
            // TODO: Support send/receive timeout config (tcpConfig)

            //Make a small queue to start.
            GrowIdCache(1000);

            this.maxMessageSize = maxMessageSize;
            this.bufferPool = bufferPool;
        }

        [Serializable]
        public struct OfferResponse
        {
            public ConnectionId connId;
            public string sdp;
            public string[] candidates;

            public OfferResponse(ConnectionId id, string offer, string[] candidates)
            {
                this.connId = id;
                this.sdp = offer;
                this.candidates = candidates;
            }
        }

        List<Common.ICEServer> iceServers;

        [Obsolete]
        public void Listen(List<Common.ICEServer> iceServers, int port, string origin)
        {
            Listen(iceServers, port, origin, false);
        }

        public void Listen(List<Common.ICEServer> iceServers, int port, string origin, bool suppressCorsHeaders)
        {
            this.iceServers = iceServers;
            allowedOrigin = origin;
            this.suppressCorsHeaders = suppressCorsHeaders;

            listener = new HttpListener();
            listener.Prefixes.Add("http://" + "*:" + port + "/");
            listener.Start();
            Log.Info($"Server has started on port {port}");

            acceptThread = new Thread(acceptLoop);
            acceptThread.IsBackground = true;
            acceptThread.Start();
        }
        List<Thread> offerAnswerThreads = new List<Thread>();
        async void acceptLoop()
        {
            try
            {
                try
                {
                    while (true)
                    {
                        HttpListenerContext ctx = await listener.GetContextAsync();
                        HttpListenerRequest req = ctx.Request;
                        HttpListenerResponse resp = ctx.Response;

                        if (req.Url.AbsolutePath.Contains("/offer/"))
                        {
                            if (!suppressCorsHeaders)
                            {
                                resp.Headers["Access-Control-Allow-Origin"] = allowedOrigin;
                            }
                            resp.Headers["Access-Control-Allow-Methods"] = "GET";
                            resp.Headers["Access-Control-Allow-Headers"] = "Content-Type";
                            resp.Headers["Content-Type"] = "application/json";

                            if (req.HttpMethod == "OPTIONS")
                            {
                                resp.Close();
                                continue;
                            }

                            Connection conn = new Connection(iceServers, GetNextId(), maxMessageSize, req.RemoteEndPoint.ToString(), AfterConnectionDisposed);
                            Connection.Config receiveConfig = new Connection.Config(
                                    conn,
                                    maxMessageSize,
                                    receiveQueue,
                                    bufferPool);
                            conn.receiveConfig = receiveConfig;
                            Log.Info($"A client connected with ID {conn.connId}");

                            // Needs its own thread as it needs to send an offer response
                            Thread offerThread = new Thread(() => GetOfferThread(conn, req, resp));
                            offerAnswerThreads.Add(offerThread);
                            offerThread.IsBackground = true;
                            offerThread.Start();
                        }
                        else if (req.Url.AbsolutePath.Contains("/answer/"))
                        {
                            if (!suppressCorsHeaders)
                            {
                                resp.Headers["Access-Control-Allow-Origin"] = allowedOrigin;
                            }
                            resp.Headers["Access-Control-Allow-Methods"] = "POST";
                            resp.Headers["Access-Control-Allow-Headers"] = "Content-Type";
                            resp.Headers["Content-Type"] = "application/json";

                            if (req.HttpMethod == "OPTIONS")
                            {
                                resp.Close();
                                continue;
                            }

                            // Needs its own thread as it needs to send an answer response 
                            Thread answerThread = new Thread(() => SendAnswerThread(req, resp));
                            offerAnswerThreads.Add(answerThread);
                            answerThread.IsBackground = true;
                            answerThread.Start();
                        }
                        else
                        {
                            resp.StatusCode = 400;
                            resp.Close();
                        }
                    }
                }
                catch (SocketException)
                {
                    // check for Interrupted/Abort
                    Common.CheckForInterrupt();
                    throw;
                }
            }
            catch (ThreadInterruptedException e) { Log.InfoException(e); }
            catch (ThreadAbortException e) { Log.InfoException(e); }
            catch (Exception e) { Log.Exception(e); }
        }

        public void Stop()
        {
            serverStopped = true;

            // Interrupt then stop so that Exception is handled correctly
            acceptThread?.Interrupt();
            listener?.Stop();
            acceptThread = null;

            for (int i = 0; i < offerAnswerThreads.Count; i++)
            {
                Thread r = offerAnswerThreads[i];
                r.Interrupt();
            }
            offerAnswerThreads.Clear();

            Log.Info("Server stopped, Closing all connections...");
            // make copy so that foreach doesn't break if values are removed
            Connection[] connectionsCopy = connections.Values.ToArray();
            foreach (Connection conn in connectionsCopy)
            {
                conn.Dispose();
            }

            connections.Clear();
        }

        void GetOfferThread(Connection conn, HttpListenerRequest req, HttpListenerResponse resp)
        {
            bool dispose = false;

            try
            {
                RTCOfferAnswerOptions options = new RTCOfferAnswerOptions();
                RTCSessionDescriptionAsyncOperation offerOp = conn.client.CreateOffer(ref options);
                while (offerOp.MoveNext())
                {

                }
                RTCSessionDescription desc = offerOp.Desc;
                var localDescOp = conn.client.SetLocalDescription(ref desc);
                while (localDescOp.MoveNext())
                {

                }

                connections.TryAdd(conn.connId, conn);

                string sdp = offerOp.Desc.sdp;
                OfferResponse respObject = new OfferResponse(conn.connId, sdp, null);
                string jsonResponse = JsonUtility.ToJson(respObject);
                byte[] respBytes = Encoding.UTF8.GetBytes(jsonResponse);

                resp.Close(respBytes, false);
            }
            catch (ThreadInterruptedException e)
            {
                dispose = true;
                Log.InfoException(e);
            }
            catch (ThreadAbortException e)
            {
                dispose = true;
                Log.InfoException(e);
            }
            catch (Exception e)
            {
                dispose = true;
                Log.Exception(e);
            }

            if (dispose)
            {
                Log.Warn("Closing connection due to exception");
                conn.Dispose();
            }
        }
        void SendAnswerThread(HttpListenerRequest req, HttpListenerResponse resp)
        {
            try
            {
                string text = "";
                using (var reader = new StreamReader(req.InputStream,
                         req.ContentEncoding))
                {
                    text = reader.ReadToEnd();
                }

                OfferResponse answer = JsonUtility.FromJson<OfferResponse>(text);
                connections.TryGetValue(answer.connId, out Connection conn);

                if (conn == null)
                {
                    Log.Error("SendAnswer: Connection " + answer.connId + " not found!");
                    return;
                }

                bool dispose = false;

                try
                {
                    RTCSessionDescription answerDescription = new RTCSessionDescription()
                    {
                        sdp = answer.sdp,
                        type = RTCSdpType.Answer
                    };
                    RTCSetSessionDescriptionAsyncOperation answerOp =
                        conn.client.SetRemoteDescription(ref answerDescription);
                    while (answerOp.MoveNext())
                    {

                    }

                    foreach (string c in answer.candidates)
                    {
                        // Skip empty candidates (end-of-candidates signal)
                        if (string.IsNullOrEmpty(c))
                            continue;
        
                        var i = new RTCIceCandidateInit();
                        i.candidate = c;
                        i.sdpMid = "0";
                        i.sdpMLineIndex = 0;
                        conn.client.AddIceCandidate(new RTCIceCandidate(i));
                    }

                    // A host candidate normally arrives first but is not usable by a
                    // native client outside the Cloud network. Wait for gathering to
                    // complete so STUN-reflexive or TURN relay candidates are returned.
                    int waited = 0;
                    while (waited < 5000)
                    {
                        if (conn.client.GatheringState == RTCIceGatheringState.Complete)
                            break;
                        Thread.Sleep(50);
                        waited += 50;
                    }

                    string output = "{\n" + "\"candidates\": [";

                    lock (conn.iceCandidates)
                    {
                        if (conn.iceCandidates.Count == 0)
                            Log.Error("ICE gathering completed without any candidates after " + waited + "ms");

                        for (int i = 0; i < conn.iceCandidates.Count; i++)
                        {
                            string c = conn.iceCandidates[i].Candidate;
                            output += "\"" + c + (i != conn.iceCandidates.Count - 1 ? "\"," : "\"");
                        }
                    }

                    output += "]\n}";

                    resp.Close(Encoding.UTF8.GetBytes(output), false);

                    // check if Stop has been called since accepting this client
                    if (serverStopped)
                    {
                        Log.Info("Server stops after successful handshake");
                        return;
                    }

                    receiveQueue.Enqueue(new Message(conn.connId, Common.EventType.Connected));
                }
                catch (ThreadInterruptedException e)
                {
                    dispose = true;
                    Log.InfoException(e);
                }
                catch (ThreadAbortException e)
                {
                    dispose = true;
                    Log.InfoException(e);
                }
                catch (Exception e)
                {
                    dispose = true;
                    Log.Exception(e);
                }

                if (dispose)
                {
                    Log.Warn("Closing connection due to exception");
                    conn.Dispose();
                }
            }
            catch (ThreadInterruptedException e) { Log.InfoException(e); }
            catch (ThreadAbortException e) { Log.InfoException(e); }
            catch (Exception e) { Log.Exception(e); }
        }

        void AfterConnectionDisposed(Connection conn)
        {
            receiveQueue.Enqueue(new Message(conn.connId, Common.EventType.Disconnected));
            connections.TryRemove(conn.connId, out Connection _);

            _idCache.Enqueue(conn.connId);
        }

        public void Send(int id, ArrayBuffer buffer, Common.DeliveryMethod deliveryMethod)
        {
            if (connections.TryGetValue(id, out Connection conn))
            {
                if (deliveryMethod == Common.DeliveryMethod.ReliableOrdered)
                {
                    conn.reliableSendQueue.Enqueue(buffer);
                    conn.reliableSendPending.Set();
                }
                else
                {
                    conn.unreliableSendQueue.Enqueue(buffer);
                    conn.unreliableSendPending.Set();
                }
            }
            else
            {
                Log.Warn($"Cant send message to {id} because connection was not found in dictionary. Maybe it disconnected.");
            }
        }

        public bool CloseConnection(int id)
        {
            if (connections.TryGetValue(id, out Connection conn))
            {
                Log.Info($"Kicking connection {id}");
                conn.Dispose();
                return true;
            }
            else
            {
                Log.Warn($"Failed to kick {id} because id not found");

                return false;
            }
        }

        public string GetClientAddress(int id)
        {
            if (connections.TryGetValue(id, out Connection conn))
            {
                return conn.clientAddress;
            }
            else
            {
                Log.Error($"Cant close connection to {id} because connection was not found in dictionary");
                return null;
            }
        }
    }
}

#endif
