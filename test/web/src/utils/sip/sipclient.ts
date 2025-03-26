import {URI} from "sip.js";

export abstract class SipClient {
    private id: string;
    private userName: string;
    private password: string;
    private uri: URI;

    constructor(id: string, uri: URI, userName: string, password: string) {
        this.id = id;
        this.uri = uri;
        this.userName = userName;
        this.password = password;
    }

    abstract run(): Promise<void>;
}

class SipTcpClient extends SipClient {

    async run() {
    }

}

class SipUdpClient extends SipClient {

    async run() {
    }

}

class SipWsClient extends SipClient {

    async run() {
    }

}

export function CreateSipClient(id: string, uri: URI, userName: string, password: string): SipClient | null {
    switch (uri.scheme) {
        case 'udp':
        {
            return new SipUdpClient(id, uri, userName, password);
        }
        case 'tcp':
        {
            return new SipTcpClient(id, uri, userName, password);
        }
        case 'ws':
        {
            return new SipWsClient(id, uri, userName, password);
        }
    }
    return null;
}
