import {initApp} from '@app/bootstrap';

initApp().catch(reason => console.error('Unexpected frontend initialization failure', reason));
